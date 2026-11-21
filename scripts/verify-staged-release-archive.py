"""Compare every decompressed ZIP file with a retained staging tree; never execute it."""
import argparse
import hashlib
import json
from pathlib import Path
import zipfile

def digest(stream):
    return hashlib.file_digest(stream, 'sha256').hexdigest()

def verify(staging, archive_path, archive_root=None):
    staging = staging.resolve(strict=True)
    root = archive_root or staging.name
    if '/' in root or '\\' in root or root in ('.', '..') or not root:
        raise ValueError('Archive root must be a single directory name')
    files = {f'{root}/{p.relative_to(staging).as_posix()}': p
             for p in staging.rglob('*') if p.is_file()}
    if not files:
        raise ValueError('Empty staging tree')
    evidence = []
    with zipfile.ZipFile(archive_path) as archive:
        members = [item.filename for item in archive.infolist() if not item.is_dir()]
        if len(members) != len(set(members)) or set(members) != set(files):
            raise ValueError('Archive has missing, extra or duplicate files')
        for name, source in sorted(files.items()):
            with source.open('rb') as stream:
                source_hash = digest(stream)
            with archive.open(name) as stream:
                archived_hash = digest(stream)
            if source_hash != archived_hash or source.stat().st_size != archive.getinfo(name).file_size:
                raise ValueError(f'Archive differs: {name}')
            evidence.append(dict(name=name, bytes=source.stat().st_size, sha256=source_hash))
    with archive_path.open('rb') as stream:
        archive_hash = digest(stream)
    return dict(archive=str(archive_path.resolve()), sha256=archive_hash, files=evidence,
                scope='Decompressed archive matches staging; no runtime or release approval')

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--staging', type=Path, required=True)
    parser.add_argument('--archive', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--archive-root', help='ZIP top directory when comparing the unchanged package source after staging cleanup')
    args = parser.parse_args()
    result = verify(args.staging, args.archive, args.archive_root)
    with args.output.open('x', encoding='utf-8') as output:
        json.dump(result, output, ensure_ascii=False, indent=2)
    print(json.dumps(dict(files=len(result['files']), sha256=result['sha256'], evidence=str(args.output))))
