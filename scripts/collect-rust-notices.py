"""Assemble a review bundle from hash-checked Cargo and pinned upstream notices.

This consumes an explicit dependency snapshot, not a binary linkage analysis.
Missing notices fail by default; audit-only output is never release-ready.
"""
import argparse
import hashlib
import json
import tomllib
from pathlib import Path, PurePosixPath


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def read_notice(root, record):
    name = record['file']
    relative = PurePosixPath(name)
    if not name or '\\' in name or ':' in name or relative.is_absolute() or any(p in ('', '.', '..') for p in name.split('/')):
        raise ValueError(f'Invalid notice path: {name}')
    path = (root / name).resolve()
    if not path.is_relative_to(root.resolve()):
        raise ValueError(f'Notice escapes source directory: {name}')
    raw = path.read_bytes()
    if digest(raw) != record['sha256']:
        raise ValueError(f'Notice content changed: {name}')
    return raw.decode('utf-8-sig')


def assemble(metadata_path, inventory_path, upstream_path, declared_terms_path=None):
    metadata_raw = metadata_path.read_bytes()
    inventory_raw = inventory_path.read_bytes()
    upstream_raw = upstream_path.read_bytes()
    metadata = json.loads(metadata_raw.decode('utf-8-sig'))
    inventory = json.loads(inventory_raw)
    upstream = json.loads(upstream_raw)
    if inventory['metadata_sha256'] != digest(metadata_raw):
        raise ValueError('Dependency inventory does not match Cargo metadata')
    packages = {}
    for package in metadata['packages']:
        key = (package['name'], package['version'], package['source'])
        if key in packages:
            raise ValueError(f'Ambiguous Cargo package: {key}')
        packages[key] = package
    text = ['UNVAC Rust dependency notices — review bundle\n',
            inventory['scope'] + '\n',
            'This bundle does not establish binary coverage or complete release obligations.\n']
    records, missing = [], []
    for row in inventory['packages']:
        if not row['source']:
            continue
        key = (row['name'], row['version'], row['source'])
        if key not in packages:
            raise ValueError(f'Inventory package absent from Cargo metadata: {key}')
        package = packages[key]
        notices = []
        if row['notices']:
            root = Path(package['manifest_path']).parent
            for item in row['notices']:
                content = read_notice(root, item)
                if content.strip():
                    notices.append((item, content, 'Cargo package'))
        if not notices:
            for source in upstream['sources']:
                if not any((c['name'], c['version']) == key[:2] for c in source['matching_crates']):
                    continue
                # A name/version match alone cannot authorize another source revision.
                vcs_path = Path(package['manifest_path']).parent / '.cargo_vcs_info.json'
                commit = json.loads(vcs_path.read_text(encoding='utf-8'))['git']['sha1'] if vcs_path.is_file() else row['source'].rsplit('#', 1)[-1]
                if commit != source['commit']:
                    raise ValueError(f'Upstream commit differs from Cargo package: {key}')
                for item in source['notices']:
                    content = read_notice(upstream_path.parent, item)
                    if content.strip():
                        notices.append((item, content, source.get('repository', '') + '@' + commit))
        if not notices:
            missing.append({'name': key[0], 'version': key[1], 'source': key[2], 'declared_license': row['license']})
            continue
        text.append('\n' + '=' * 72 + f'\n{key[0]} {key[1]}\nSource: {key[2]}\nDeclared license: {row["license"]}\n')
        copied = []
        for item, content, origin in notices:
            text.append(f'\n--- {item["file"]} ({origin}) ---\n{content}\n')
            copied.append({'file': item['file'], 'sha256': item['sha256'], 'origin': origin})
        records.append({'name': key[0], 'version': key[1], 'source': key[2], 'notices': copied})
    declared_terms = []
    declared_terms_source = None
    if declared_terms_path is not None:
        declarations = json.loads(declared_terms_path.read_text(encoding='utf-8'))
        terms_text = read_notice(declared_terms_path.parent, declarations['terms'])
        declared_terms_source = dict(declaration_sha256=digest(declared_terms_path.read_bytes()), terms=declarations['terms'])
        if not terms_text.strip():
            raise ValueError('Standard terms are empty')
        allowed = {'MIT OR Apache-2.0', 'Apache-2.0/MIT', 'MIT/Apache-2.0', 'EPL-2.0 OR Apache-2.0'}
        seen = set()
        for declaration in declarations['packages']:
            key = (declaration['name'], declaration['version'], declaration['source'])
            if key in seen or key not in packages:
                raise ValueError(f'Duplicate or unknown standard terms package: {key}')
            seen.add(key)
            package = packages[key]
            manifest_raw = Path(package['manifest_path']).read_bytes()
            manifest = tomllib.loads(manifest_raw.decode('utf-8-sig'))['package']
            if digest(manifest_raw) != declaration['manifest_sha256']:
                raise ValueError(f'License declaration manifest changed: {key}')
            expression = declaration['declared_license']
            if (expression not in allowed or declaration['selected_terms'] != 'Apache-2.0'
                    or manifest.get('license') != expression or package.get('license') != expression
                    or (manifest['name'], manifest['version']) != key[:2]):
                raise ValueError(f'Unsupported or mismatched license alternative: {key}')
            if declaration['authors_as_declared'] != manifest.get('authors', []):
                raise ValueError(f'Declared authors changed: {key}')
            for excerpt in declaration.get('source_notices', []):
                source_text = read_notice(Path(package['manifest_path']).parent, excerpt)
                start, end = excerpt['start_line'], excerpt['end_line']
                lines = source_text.splitlines(keepends=True)
                if (type(start) is not int or type(end) is not int
                        or not 1 <= start <= end <= len(lines)):
                    raise ValueError(f'Invalid source notice range: {key}')
                actual = ''.join(lines[start - 1:end])
                if not actual.strip() or actual != excerpt['text']:
                    raise ValueError(f'Source notice excerpt changed: {key}')
            declared_terms.append(declaration)
        text.append('\n' + '=' * 72 + '\nDeclared Apache-2.0 alternatives — standard terms supplement\n'
                    'These declarations do not replace upstream attribution or NOTICE review.\n')
        for declaration in declared_terms:
            text.append(f"\n{declaration['name']} {declaration['version']}\n"
                        f"Source: {declaration['source']}\nDeclared: {declaration['declared_license']}\n"
                        f"Authors as declared (not inferred copyright holders): {json.dumps(declaration['authors_as_declared'], ensure_ascii=False)}\n")
            for excerpt in declaration.get('source_notices', []):
                text.append(f"Source notice preserved from {excerpt['file']} "
                            f"lines {excerpt['start_line']}-{excerpt['end_line']}:\n{excerpt['text']}\n")
        text.append('\n' + terms_text + '\n')
    raw = ''.join(text).encode('utf-8')
    report = dict(schema_version=1, release_ready=False,
                  scope=inventory['scope'], metadata_sha256=digest(metadata_raw),
                  inventory_sha256=digest(inventory_raw), upstream_sha256=digest(upstream_raw),
                  notice_sha256=digest(raw), packages=records, unresolved=missing,
                  declared_standard_terms=declared_terms,
                  declared_standard_terms_source=declared_terms_source,
                  remaining_gate='Verify current release dependency coverage and embedded native-code obligations before packaging.')
    return raw, report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--metadata', type=Path, required=True)
    parser.add_argument('--inventory', type=Path, required=True)
    parser.add_argument('--upstream', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--declared-terms', type=Path, help='Optional audited license-declaration supplement; does not clear missing upstream notices')
    parser.add_argument('--audit-only', action='store_true', help='Permit incomplete review output, never mark it ready')
    args = parser.parse_args()
    raw, report = assemble(args.metadata, args.inventory, args.upstream, args.declared_terms)
    args.output.mkdir(parents=True, exist_ok=False)
    (args.output / 'THIRD-PARTY-RUST.review.txt').write_bytes(raw)
    (args.output / 'rust-notices.review.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(f'{len(report["packages"])} packages with verified notice text; {len(report["unresolved"])} unresolved; review only')
    if report['unresolved'] and not args.audit_only:
        raise SystemExit('Missing notices: no release-ready bundle generated')


if __name__ == '__main__':
    main()
