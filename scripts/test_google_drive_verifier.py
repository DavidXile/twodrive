"""Run the Google verifier's no-mount branch against a disposable IPC substitute."""
import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('google_verifier', Path(__file__).with_name('verify-google-drive.py'))
verifier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verifier)


class DisposableEngine:
    def __init__(self, *args):
        self.files = {}
        self.directories = set()
        self.folders = []
        self.version = 0

    def call(self, op, args=None, failure=False, **kwargs):
        args = args or {}
        if op in ('save_profile', 'connect'):
            return {}
        if op == 'profiles':
            return {'providers': [{'id': 'google'}]}
        if op == 'mkdir':
            self.directories.add(args['path'])
            return {}
        if op == 'upload':
            path = args['path']
            existing = self.files.get(path)
            conflict = existing is not None and existing[1] != args.get('etag')
            if failure:
                assert conflict
                return 'version conflict'
            assert not conflict
            self.version += 1
            self.files[path] = (Path(args['source']).read_bytes(), str(self.version))
            return {'path': path, 'etag': str(self.version)}
        if op == 'download':
            Path(args['output']).write_bytes(self.files[args['path']][0])
            return {}
        if op == 'rename':
            self.files[args['to']] = self.files.pop(args['path'])
            return {}
        if op == 'list':
            path = args['path']
            entries = [{'name': Path(p).name, 'directory': True}
                       for p in self.directories if str(Path(p).parent) == path]
            entries += [{'name': Path(p).name, 'directory': False, 'etag': etag}
                        for p, (_, etag) in self.files.items() if str(Path(p).parent) == path]
            return {'entries': entries}
        if op == 'save_backups':
            self.folders = args['folders']
            return {}
        if op == 'status':
            for folder in self.folders:
                self.directories.add(folder['remote'])
                for local in Path(folder['local']).iterdir():
                    data = local.read_bytes()
                    remote = folder['remote'] + '/' + local.name
                    if self.files.get(remote, (None, None))[0] != data:
                        self.version += 1
                        self.files[remote] = (data, str(self.version))
            return {'backup': {'state': 'idle', 'completed_scan': True}}
        if op == 'delete':
            path = args['path']
            self.files = {p: v for p, v in self.files.items() if p != path and not p.startswith(path + '/')}
            self.directories = {p for p in self.directories if p != path and not p.startswith(path + '/')}
            return {}
        raise AssertionError('unexpected operation: ' + op)

    def close(self):
        assert not self.files and not self.directories, 'owned substitute data must be cleaned'


class VerifierTests(unittest.TestCase):
    def test_without_mount_finishes_backup_and_owned_cleanup(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args = SimpleNamespace(engine=root / 'engine', peer=root / 'peer', client_file=root / 'client.json',
                google_state=root / 'private', output=root / 'report.json', mount=False, bytes=17)
            with patch.object(verifier, 'Engine', DisposableEngine), patch('builtins.print'):
                verifier.run(args)
            result = json.loads(args.output.read_text())
            self.assertTrue(result['checks']['folder_backup_create_update'])
            self.assertTrue(result['checks']['owned_remote_folder_trashed'])
            self.assertNotIn('fuse_read', result['checks'])


if __name__ == '__main__':
    unittest.main()
