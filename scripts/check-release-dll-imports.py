"""Inspect Windows imports without loading the candidate or searching developer PATH.

This is static dependency evidence, not clean-machine or delayed-load acceptance.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess


def inspect(root, dumpbin):
    local = {p.name.lower(): p for p in root.iterdir()
             if p.is_file() and p.suffix.lower() in ('.exe', '.dll')}
    if not any(p.suffix.lower() == '.exe' for p in local.values()):
        raise ValueError('No executable in candidate directory')
    system = Path(os.environ['SystemRoot']) / 'System32'
    files = []
    unresolved = []
    redistributables = set()
    for name, path in sorted(local.items()):
        proc = subprocess.run([str(dumpbin), '/nologo', '/dependents', str(path)],
                              capture_output=True, timeout=30, creationflags=subprocess.CREATE_NO_WINDOW)
        output = proc.stdout.decode('utf-8', errors='replace')
        if proc.returncode:
            raise ValueError(f'dumpbin failed for {name}: exit {proc.returncode}')
        dependencies = []
        for dependency in sorted(set(re.findall(r'^\s+([\w.\-]+\.dll)\s*$', output, re.M | re.I))):
            key = dependency.lower()
            if key in local:
                resolution = 'bundled'
            elif key.startswith(('api-ms-win-', 'ext-ms-win-')):
                resolution = 'windows-api-set'
            elif (system / dependency).is_file():
                resolution = ('present-in-host-driver-runtime' if key == 'vulkan-1.dll'
                              else 'present-in-host-system32')
                if key.startswith(('vcruntime', 'msvcp', 'concrt', 'vcomp')):
                    redistributables.add(dependency)
            else:
                resolution = 'unresolved-without-developer-path'
                unresolved.append({'file': name, 'dependency': dependency})
            dependencies.append({'name': dependency, 'resolution': resolution})
        if not dependencies:
            raise ValueError(f'No imports parsed for {name}; inspect dumpbin output before accepting')
        with path.open('rb') as binary:
            digest = hashlib.file_digest(binary, 'sha256').hexdigest()
        files.append({'file': name, 'bytes': path.stat().st_size,
                      'sha256': digest,
                      'dependencies': dependencies})
    return {'schema_version': 1, 'root': str(root), 'files': files,
            'unresolved': unresolved, 'host_vc_runtime_dependencies': sorted(redistributables),
            'limits': ['No binaries loaded or executed', 'Host System32 presence is not proof of clean-machine availability',
                       'Runtime LoadLibrary dependencies and minimum Windows version require separate checks']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--dumpbin', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Evidence output already exists; choose a new file')
    result = inspect(args.root.resolve(), args.dumpbin.resolve())
    with args.output.open('x', encoding='utf-8') as output:
        json.dump(result, output, ensure_ascii=False, indent=2)
    print(f"Inspected {len(result['files'])} binaries; unresolved {len(result['unresolved'])}; "
          f"host VC runtime dependencies {len(result['host_vc_runtime_dependencies'])}")
    return 1 if result['unresolved'] else 0


if __name__ == '__main__':
    raise SystemExit(main())
