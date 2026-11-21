"""Synthetic failure cases for the review-bundle collector; no network or app."""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile

spec = importlib.util.spec_from_file_location('collector', Path(__file__).with_name('collect-rust-notices.py'))
collector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(collector)
with tempfile.TemporaryDirectory(prefix='unvac-notice-') as tmp:
    root = Path(tmp)
    source = root / 'crate'
    source.mkdir()
    notice = source / 'LICENSE'
    notice.write_bytes(b'Synthetic notice\n')
    metadata = root / 'metadata.json'
    inventory = root / 'inventory.json'
    upstream = root / 'upstream.json'
    package = dict(name='example', version='1.0.0', source='registry+fixture', manifest_path=str(source / 'Cargo.toml'))
    metadata.write_text(json.dumps({'packages': [package]}))
    row = {**package, 'license': 'MIT', 'notices': [dict(file='LICENSE', sha256=collector.digest(notice.read_bytes()))]}
    inv = dict(scope='Synthetic test only', metadata_sha256=collector.digest(metadata.read_bytes()), packages=[row])
    def save():
        inventory.write_text(json.dumps(inv))
    upstream.write_text(json.dumps({'sources': []}))
    save()
    raw, report = collector.assemble(metadata, inventory, upstream)
    assert b'Synthetic notice' in raw and not report['unresolved'] and not report['release_ready']
    notice.write_bytes(b'Changed')
    try: collector.assemble(metadata, inventory, upstream)
    except ValueError: pass
    else: raise AssertionError('Changed source accepted')
    notice.write_bytes(b'')
    row['notices'][0]['sha256'] = collector.digest(b'')
    save()
    assert len(collector.assemble(metadata, inventory, upstream)[1]['unresolved']) == 1
    result = subprocess.run([sys.executable, str(Path(collector.__file__)), '--metadata', str(metadata),
        '--inventory', str(inventory), '--upstream', str(upstream), '--output', str(root/'out')], capture_output=True)
    assert result.returncode != 0 and (root/'out/rust-notices.review.json').exists()
    row['notices'][0]['file'] = '../outside'
    save()
    try: collector.assemble(metadata, inventory, upstream)
    except ValueError: pass
    else: raise AssertionError('Traversal accepted')
    row['notices'] = []
    save()
    (source/'.cargo_vcs_info.json').write_text(json.dumps({'git': {'sha1': 'a'*40}}))
    upstream.write_text(json.dumps({'sources': [dict(commit='b'*40, matching_crates=[package], notices=[])]}))
    try: collector.assemble(metadata, inventory, upstream)
    except ValueError: pass
    else: raise AssertionError('Wrong upstream revision accepted')
    inv['metadata_sha256'] = '0'*64
    save()
    try: collector.assemble(metadata, inventory, upstream)
    except ValueError: pass
    else: raise AssertionError('Different dependency snapshot accepted')
    # Standard terms are supplemental: an OR declaration must not silently clear
    # a missing upstream NOTICE, nor authorize a different/AND license.
    manifest = source/'Cargo.toml'
    manifest.write_text('[package]\nname="example"\nversion="1.0.0"\nlicense="MIT OR Apache-2.0"\nauthors=["Example Author"]\n')
    package['license'] = 'MIT OR Apache-2.0'
    metadata.write_text(json.dumps({'packages':[package]}))
    inv['metadata_sha256'] = collector.digest(metadata.read_bytes())
    row['license'] = package['license']
    save()
    upstream.write_text(json.dumps({'sources':[]}))
    standard = root/'standard.txt'
    standard.write_text('Synthetic standard terms fixture')
    supplement = root/'declared.json'
    entry = dict(name='example',version='1.0.0',source='registry+fixture',
        declared_license=package['license'], selected_terms='Apache-2.0',
        manifest_sha256=collector.digest(manifest.read_bytes()),authors_as_declared=['Example Author'])
    declaration = dict(terms=dict(file='standard.txt',sha256=collector.digest(standard.read_bytes())),packages=[entry])
    def save_declaration(): supplement.write_text(json.dumps(declaration))
    save_declaration()
    raw, report = collector.assemble(metadata, inventory, upstream, supplement)
    assert b'Synthetic standard terms fixture' in raw
    assert len(report['unresolved']) == 1 and len(report['declared_standard_terms']) == 1
    assert not report['release_ready']
    entry['declared_license'] = 'MIT AND Apache-2.0'
    save_declaration()
    try: collector.assemble(metadata, inventory, upstream, supplement)
    except ValueError: pass
    else: raise AssertionError('AND declaration accepted as an alternative')
    entry['declared_license'] = package['license']
    entry['authors_as_declared'] = ['Invented Owner']
    save_declaration()
    try: collector.assemble(metadata, inventory, upstream, supplement)
    except ValueError: pass
    else: raise AssertionError('Invented attribution accepted')
    entry['authors_as_declared'] = ['Example Author']
    header = source/'lib.rs'
    header.write_bytes(b'// Copyright Synthetic Holder\n// License declaration\nfn main() {}\n')
    excerpt = dict(file='lib.rs',sha256=collector.digest(header.read_bytes()),
                   start_line=1,end_line=2,text='// Copyright Synthetic Holder\n// License declaration\n')
    entry['source_notices'] = [excerpt]
    save_declaration()
    raw, report = collector.assemble(metadata, inventory, upstream, supplement)
    assert excerpt['text'].encode() in raw and len(report['unresolved']) == 1
    excerpt['text'] = '// Invented holder\n'
    save_declaration()
    try: collector.assemble(metadata, inventory, upstream, supplement)
    except ValueError: pass
    else: raise AssertionError('Invented source notice accepted')
    excerpt['text'] = '// Copyright Synthetic Holder\n// License declaration\n'
    excerpt['end_line'] = 100
    save_declaration()
    try: collector.assemble(metadata, inventory, upstream, supplement)
    except ValueError: pass
    else: raise AssertionError('Out-of-range source notice accepted')
    excerpt['end_line'] = 2
    header.write_bytes(header.read_bytes()+b'// changed\n')
    save_declaration()
    try: collector.assemble(metadata, inventory, upstream, supplement)
    except ValueError: pass
    else: raise AssertionError('Changed source file accepted')
    entry['source_notices'] = []
    save_declaration()
    manifest.write_text(manifest.read_text()+'# modified\n')
    try: collector.assemble(metadata, inventory, upstream, supplement)
    except ValueError: pass
    else: raise AssertionError('Changed license declaration accepted')
print('PASS: notice integrity, missing notices, source revision, standard terms alternatives, attribution and manifest guards')
