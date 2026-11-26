"""Keep failed native process startup from retaining the fixture's log file."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('desktop_fixture', Path(__file__).with_name('test-dav-desktop.py'))
desktop = importlib.util.module_from_spec(spec)
spec.loader.exec_module(desktop)


class EngineFixtureTests(unittest.TestCase):
    def test_failed_create_process_closes_stderr(self):
        with tempfile.TemporaryDirectory() as root:
            log = Path(root, 'engine.stderr').open('w')
            try:
                with patch.object(Path, 'open', return_value=log), patch.object(
                    desktop.subprocess, 'Popen', side_effect=PermissionError('controlled process denial'),
                ):
                    with self.assertRaises(PermissionError):
                        desktop.Engine('unused-engine', 'unused-peer', Path(root))
                    self.assertTrue(log.closed)
            finally:
                log.close()


if __name__ == '__main__':
    unittest.main()
