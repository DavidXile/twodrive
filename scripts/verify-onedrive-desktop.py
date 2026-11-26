#!/usr/bin/env python3
"""Live OneDrive desktop verification in an explicitly supplied private account.

Only creates/modifies/deletes its own random remote directory; never logs tokens
or personal file listings. Does not operate the installed stable daemon.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import uuid

spec = importlib.util.spec_from_file_location('desktop_check', Path(__file__).with_name('test-dav-desktop.py'))
desktop = importlib.util.module_from_spec(spec)
spec.loader.exec_module(desktop)


def run(args):
    checks = {}
    with tempfile.TemporaryDirectory(prefix='td-onedrive-desktop-') as tmp:
        root = Path(tmp)
        engine = desktop.Engine(str(args.engine.resolve()), str(args.peer.resolve()), root)
        remote = '/twodrive-verification-' + uuid.uuid4().hex
        created = False
        try:
            engine.call('save_profile', dict(id='od-live', name='Disposable verification',
                kind='onedrive', onedrive_state=str(args.onedrive_state.resolve()),
                network_route='system', writable=True, mount_path=str(root/'mount')))
            engine.call('connect', {'id': 'od-live'}, timeout=120)
            checks['actual_saved_login_and_identity_probe'] = True
            engine.call('mkdir', {'path': remote})
            created = True
            source = root/'source.bin'
            data = (bytes(range(251)) * (args.bytes//251+1))[:args.bytes]
            source.write_bytes(data)
            item = engine.call('upload', {'path': remote+'/样本.bin', 'source': str(source)}, timeout=240)
            engine.call('upload', {'path': item['path'], 'source': str(source)}, failure=True)
            out = root/'download.bin'
            engine.call('download', {'path': item['path'], 'output': str(out)}, timeout=240)
            assert out.read_bytes() == data
            checks.update(upload_bytes=len(data), download_sha256=hashlib.sha256(data).hexdigest(),
                          create_only=True)
            source.write_bytes(b'conditional overwrite\n')
            changed = engine.call('upload', {'path': item['path'], 'source': str(source), 'etag': item['etag']})
            engine.call('upload', {'path': item['path'], 'source': str(source), 'etag': item['etag']}, failure=True)
            engine.call('rename', {'path': changed['path'], 'to': remote+'/renamed.txt'})
            small = root/'changed.txt'
            engine.call('download', {'path': remote+'/renamed.txt', 'output': str(small)})
            assert small.read_bytes() == source.read_bytes()
            checks.update(conditional_overwrite=True, stale_version_rejected=True, move_rename=True)
            engine.call('delete', {'path': remote+'/renamed.txt'})
            assert not engine.call('list', {'path': remote})['entries']
            checks['delete'] = True
        finally:
            try:
                if created:
                    engine.call('delete', {'path': remote})
                    checks['owned_remote_folder_deleted'] = True
            finally:
                engine.close()
    report = dict(environment='live-OneDrive-isolated-desktop', checks=checks,
                  boundaries=['Existing saved login tested; new browser OAuth not executed.',
                              'No real OneDrive FUSE, folder backup, organization account or Windows validation.'])
    args.output.write_text(json.dumps(report, indent=2)+'\n')
    return report


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--engine', required=True, type=Path)
    parser.add_argument('--peer', required=True, type=Path)
    parser.add_argument('--onedrive-state', required=True, type=Path)
    parser.add_argument('--bytes', type=int, default=8*1024*1024+123)
    parser.add_argument('--output', required=True, type=Path)
    print(json.dumps(run(parser.parse_args()), indent=2))
