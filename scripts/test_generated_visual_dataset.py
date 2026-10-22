"""Unregistered-root decisions without registration or fixture content reads."""
import importlib.util
from pathlib import Path
import tempfile
import stat
import os
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    'visual_validator', Path(__file__).with_name('test-generated-visual-dataset.py'))
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)


class OrdinaryRootGuardTests(unittest.TestCase):
    def test_only_specific_outside_root_result_is_accepted(self):
        validator.require_unregistered(0x80070186)
        validator.require_unregistered(0x80070186 - 2**32)

    def test_registered_generic_and_ordinary_file_errors_are_refused_and_printed(self):
        # NOT_A_CLOUD_FILE describes an item, not absence of a containing root.
        for code in (0, 1, 0x80070001, 0x80070005, 0x80070032, 0x80070178):
            with self.subTest(code=code):
                with self.assertRaisesRegex(RuntimeError, f'0x{code:08x}'):
                    validator.require_unregistered(code - 2**32 if code >> 31 else code)

    def test_real_ordinary_directory_ancestry_is_checked_without_data(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'generated'
            root.mkdir()
            with patch.object(Path, 'read_bytes', side_effect=AssertionError('data read')):
                validator.require_plain_directory(root)

    def test_reparse_metadata_is_refused_before_content(self):
        for mode, attributes in ((stat.S_IFDIR, 0x400), (stat.S_IFLNK, 0)):
            with self.subTest(mode=mode):
                with patch.object(Path, 'lstat', return_value=SimpleNamespace(st_mode=mode, st_file_attributes=attributes)):
                    with self.assertRaisesRegex(RuntimeError, 'reparse ancestry'):
                        validator.require_plain_directory(Path.cwd())

    def test_file_and_missing_directory_are_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'ordinary-file'
            root.write_bytes(b'generated')
            with self.assertRaisesRegex(RuntimeError, 'directory ancestry'):
                validator.require_plain_directory(root)
            with self.assertRaises(FileNotFoundError):
                validator.require_plain_directory(root.with_name('missing'))

    def test_permission_failure_does_not_become_plain_directory(self):
        with patch.object(Path, 'lstat', side_effect=PermissionError('generated denial')):
            with self.assertRaises(PermissionError):
                validator.require_plain_directory(Path.cwd())

    @unittest.skipUnless(os.name == 'nt', 'Windows metadata API only')
    def test_real_generated_directory_must_have_known_outside_root_evidence(self):
        configured = os.environ.get('TWODRIVE_ORDINARY_FIXTURE')
        parent = Path(configured).parent if configured else None
        with tempfile.TemporaryDirectory(prefix='ordinary-cfapi-probe-', dir=parent) as directory:
            root = Path(directory)
            validator.require_plain_directory(root)
            with patch.object(Path, 'read_bytes', side_effect=AssertionError('fixture content read')) as read:
                try:
                    validator.windows_root_preflight(root)
                except validator.RootEvidenceUnavailable as error:
                    read.assert_not_called()
                    if error.hresult==0x80070001 and os.environ.get('TWODRIVE_CFAPI_INSTANCES')=='absent':
                        self.skipTest('CFAPI instance absent: outside-root evidence unavailable; no fixture content read')
                    raise


if __name__ == '__main__':
    unittest.main()
