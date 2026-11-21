"""Release ZIP publication failures must not produce a final-looking artifact."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('release_package', Path(__file__).with_name('package-release-candidate.py'))
package_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package_module)


class PublicationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='vac-package-check-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / 'source'
        self.source.mkdir()
        payload = b'not an executable; archive fixture'
        (self.source / 'app.exe').write_bytes(payload)
        inventory = {'schema_version': 1, 'variant': 'portable', 'files': {
            'app.exe': {'bytes': len(payload), 'sha256': hashlib.sha256(payload).hexdigest()}}}
        (self.source / package_module.INVENTORY).write_text(json.dumps(inventory), encoding='utf-8')
        self.output = self.root / 'candidate.zip'

    def test_verification_failure_leaves_no_final_or_partial_archive(self):
        original = package_module.zipfile.ZipFile

        class RejectVerification(original):
            def __init__(self, file, mode='r', *args, **kwargs):
                if mode == 'r':
                    raise ValueError('injected decompressed-member verification failure')
                super().__init__(file, mode, *args, **kwargs)

        with patch.object(package_module.zipfile, 'ZipFile', RejectVerification):
            with self.assertRaisesRegex(ValueError, 'injected'):
                package_module.package(self.source, self.output)
        self.assertFalse(self.output.exists())
        self.assertEqual(list(self.root.glob('*.partial')), [])

    def test_verified_archive_is_published_and_never_overwritten(self):
        result = package_module.package(self.source, self.output)
        previous = self.output.read_bytes()
        self.assertEqual(result['archive'], str(self.output.resolve()))
        with self.assertRaises(FileExistsError):
            package_module.package(self.source, self.output)
        self.assertEqual(self.output.read_bytes(), previous)

    def test_missing_manual_target_prevents_publication(self):
        manual = b'[Setup](manual/setup.md)'
        (self.source / 'README.md').write_bytes(manual)
        inventory_path = self.source / package_module.INVENTORY
        inventory = json.loads(inventory_path.read_text())
        inventory['files']['README.md'] = {'bytes': len(manual), 'sha256': hashlib.sha256(manual).hexdigest()}
        inventory_path.write_text(json.dumps(inventory))
        with self.assertRaisesRegex(ValueError, 'Missing packaged manual target'):
            package_module.package(self.source, self.output)
        self.assertFalse(self.output.exists())
        self.assertEqual(list(self.root.glob('*.partial')), [])

    def test_competing_final_creation_is_preserved(self):
        real_link = package_module.os.link

        def competing_link(source, destination):
            Path(destination).write_bytes(b'other reviewed artifact')
            return real_link(source, destination)

        with patch.object(package_module.os, 'link', competing_link):
            with self.assertRaises(FileExistsError):
                package_module.package(self.source, self.output)
        self.assertEqual(self.output.read_bytes(), b'other reviewed artifact')
        self.assertEqual(list(self.root.glob('*.partial')), [])


if __name__ == '__main__':
    unittest.main()
