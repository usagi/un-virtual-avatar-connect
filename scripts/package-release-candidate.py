"""Package an already verified release inventory; never include the build directory wholesale."""
import posixpath
import re
from urllib.parse import unquote, urlsplit
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import tempfile
import zipfile

INVENTORY = 'unvac-release-files.json'

def digest(stream):
    return hashlib.file_digest(stream, 'sha256').hexdigest()

def package(source, output):
    """Publish only a fully verified archive; never replace an existing one."""
    if output.exists():
        raise FileExistsError(output)
    fd, name = tempfile.mkstemp(prefix=output.name + '.', suffix='.partial', dir=output.parent)
    os.close(fd)
    temporary = Path(name)
    try:
        result = _package_verified(source, temporary)
        # Same-directory hard link publishes atomically and fails if the final
        # name appeared during validation. os.replace would overwrite it.
        os.link(temporary, output)
        result['archive'] = str(output.resolve())
        return result
    finally:
        temporary.unlink(missing_ok=True)


def _package_verified(source, output):
    source = source.resolve(strict=True)
    inventory_bytes = (source / INVENTORY).read_bytes()
    inventory = json.loads(inventory_bytes)
    if inventory.get('schema_version') != 1 or not inventory.get('files') or not isinstance(inventory.get('variant'), str):
        raise ValueError('Unsupported or empty inventory')
    files = inventory['files']
    for name, expected in files.items():
        parts = PurePosixPath(name).parts
        if not parts or '\\' in name or ':' in name or name.startswith('/') or any(p in ('.', '..') for p in name.split('/')):
            raise ValueError('Invalid archive member: ' + name)
        path = source.joinpath(*parts)
        if any(parent.is_symlink() for parent in [path, *path.parents] if parent != source.parent):
            raise ValueError('Symbolic links are not release inputs: ' + name)
        if not path.resolve(strict=True).is_relative_to(source):
            raise ValueError('Release input escapes directory: ' + name)
        with path.open('rb') as stream:
            if path.stat().st_size != expected['bytes'] or digest(stream) != expected['sha256']:
                raise ValueError('Release input differs: ' + name)
    # This path is an exclusively allocated temporary file, never a published ZIP.
    with output.open('wb') as target:
        with zipfile.ZipFile(target, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
            for name in sorted(files):
                archive.write(source / name, name)
            archive.writestr(INVENTORY, inventory_bytes)
    # Read every decompressed member, not just ZIP metadata/CRC.
    with zipfile.ZipFile(output) as archive:
        if set(archive.namelist()) != set(files) | {INVENTORY} or len(archive.namelist()) != len(files) + 1:
            raise ValueError('Unexpected or duplicate archive members')
        if archive.read(INVENTORY) != inventory_bytes:
            raise ValueError('Archived inventory differs')
        for name, expected in files.items():
            with archive.open(name) as stream:
                if archive.getinfo(name).file_size != expected['bytes'] or digest(stream) != expected['sha256']:
                    raise ValueError('Archived content differs: ' + name)
        manual_links = verify_manual_links(archive)
    with output.open('rb') as stream:
        sha = digest(stream)
    return {'archive': str(output.resolve()), 'sha256': sha, 'bytes': output.stat().st_size,
            'variant': inventory['variant'], 'payload_files': len(files), 'archive_members': len(files)+1,
            'verification': 'Every decompressed member matches the source inventory; no application launched',
            'manual_local_links_checked': manual_links, 'release_approved': False}

def verify_manual_links(archive):
    """Check inline local file/image links, excluding fenced examples and anchors.

    This deliberately does not claim heading, reference-style, or Web validation.
    """
    names = set(archive.namelist())
    checked = 0
    for name in sorted(names):
        if not name.lower().endswith('.md'):
            continue
        text = archive.read(name).decode('utf-8-sig')
        fence = None
        lines = []
        for line in text.splitlines():
            marker = re.match(r'^\s*(`{3,}|~{3,})', line)
            if marker:
                token = marker.group(1)
                if fence is None:
                    fence = token
                elif token[0] == fence[0] and len(token) >= len(fence):
                    fence = None
                continue
            if fence is None:
                lines.append(line)
        for match in re.finditer(r'!?\[[^\]\n]*\]\(([^\s)]+)\)', '\n'.join(lines)):
            href = urlsplit(match.group(1).strip('<>'))
            if href.scheme or href.netloc or not href.path:
                continue
            target = posixpath.normpath(posixpath.join(posixpath.dirname(name), unquote(href.path)))
            checked += 1
            if target not in names and not any(n.startswith(target.rstrip('/') + '/') for n in names):
                raise ValueError(f'Missing packaged manual target: {name} -> {target}')
    return checked

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    result = package(args.source, args.output)
    args.output.with_suffix('.verification.json').write_text(json.dumps(result, indent=2), encoding='utf-8')
    print(json.dumps(result))
