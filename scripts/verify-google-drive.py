#!/usr/bin/env python3
"""Live Google Drive IPC check using only an owned disposable remote folder.
Never logs tokens, authorization URLs, account identities or personal listings.
"""
import argparse
import hashlib
import importlib.util
import json
import os
import shutil
from pathlib import Path
import tempfile
import uuid
import time

spec = importlib.util.spec_from_file_location('desktop_check', Path(__file__).with_name('test-dav-desktop.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
Engine = module.Engine


def run(args):
    report = {'environment': 'live Google Drive', 'checks': {}, 'boundaries': [
        'Uses only its newly created disposable remote folder; delete moves it to trash.',
        'Does not prove Google editor byte editing, shared drives, Windows or every network condition.',
    ]}
    directory = tempfile.mkdtemp(prefix='td-google-check-')
    root = Path(directory)
    engine = Engine(str(args.engine.resolve()), str(args.peer.resolve()), root)
    remote = '/twodrive-verification-' + uuid.uuid4().hex
    created = False
    failure = None
    try:
        profile = {'id': 'google-verification', 'name': 'Google verification', 'kind': 'google',
            'client_file': str(args.client_file.resolve()), 'google_state': str(args.google_state.resolve()),
            'network_route': 'system', 'writable': True, 'mount_path': str(root / 'mount')}
        engine.call('save_profile', profile)
        public = engine.call('profiles')
        assert 'google' in [p['id'] for p in public['providers']]
        engine.call('connect', {'id': profile['id']}, timeout=120)
        report['checks']['connect'] = True
        engine.call('mkdir', {'path': remote})
        created = True
        engine.call('mkdir', {'path': remote + '/folder'})
        source = root / 'source.bin'
        data = (bytes(range(251)) * (args.bytes // 251 + 1))[:args.bytes]
        source.write_bytes(data)
        uploaded = engine.call('upload', {'path': remote + '/folder/样本.bin', 'source': str(source)}, timeout=240)
        engine.call('upload', {'path': uploaded['path'], 'source': str(source)}, failure=True)
        engine.call('download', {'path': uploaded['path'], 'output': str(root / 'download.bin'), 'size': len(data)}, timeout=240)
        assert (root / 'download.bin').read_bytes() == data
        report['checks'].update(upload=True, download_sha256=hashlib.sha256(data).hexdigest(), bytes=len(data), create_only=True)
        old_etag = uploaded['etag']
        replacement = b'TwoDrive Google Drive conditional overwrite verification\n'
        source.write_bytes(replacement)
        changed = engine.call('upload', {'path': uploaded['path'], 'source': str(source), 'etag': old_etag}, timeout=240)
        source.write_bytes(b'must not overwrite newer version')
        engine.call('upload', {'path': changed['path'], 'source': str(source), 'etag': old_etag}, failure=True)
        engine.call('download', {'path': changed['path'], 'output': str(root / 'changed.bin'), 'size': len(replacement)}, timeout=120)
        assert (root / 'changed.bin').read_bytes() == replacement
        report['checks'].update(overwrite=True, stale_version_rejected=True)
        engine.call('rename', {'path': changed['path'], 'to': remote + '/moved.bin'})
        listing = engine.call('list', {'path': remote})
        assert len(listing['entries']) == 2
        report['checks'].update(move_and_rename=True, unicode_name=True)
        if args.mount:
            engine.call('mount', timeout=720)
            mounted = root / 'mount' / remote.lstrip('/') / 'moved.bin'
            assert mounted.read_bytes() == replacement
            report['checks']['fuse_read'] = True
            mounted.write_bytes(b'FUSE Google Drive upload verification\n')
            for _ in range(90):
                time.sleep(1)
                cache = engine.call('cache_info', {'id': profile['id']})
                if cache['pending'] == 0:
                    break
            else:
                raise AssertionError('mounted write did not finish uploading')
            engine.call('unmount', timeout=120)
            engine.call('download', {'path': remote + '/moved.bin', 'output': str(root / 'fuse.bin')}, timeout=120)
            assert (root / 'fuse.bin').read_bytes() == b'FUSE Google Drive upload verification\n'
            report['checks']['fuse_write_cloud_roundtrip'] = True
        backup_source = root / 'backup-source'
        backup_source.mkdir()
        sample = backup_source / 'sample.txt'
        sample.write_bytes(b'first backup version')
        engine.call('save_backups', {'id': profile['id'], 'folders': [
            {'local': str(backup_source), 'remote': remote + '/backup', 'enabled': True}
        ]})
        def wait_backup(expected):
            deadline = time.monotonic() + 180
            while time.monotonic() < deadline:
                status = engine.call('status')
                assert status.get('backup', {}).get('state') != 'error', 'backup failed'
                if status.get('backup', {}).get('completed_scan'):
                    listing = engine.call('list', {'path': remote + '/backup'})
                    if any(e['name'] == 'sample.txt' for e in listing['entries']):
                        output = root / ('backup-' + uuid.uuid4().hex)
                        engine.call('download', {'path': remote + '/backup/sample.txt', 'output': str(output)}, timeout=120)
                        if output.read_bytes() == expected:
                            return
                time.sleep(2)
            raise AssertionError('backup did not complete')
        wait_backup(b'first backup version')
        sample.write_bytes(b'updated backup version')
        wait_backup(b'updated backup version')
        sample.unlink()
        engine.call('save_backups', {'id': profile['id'], 'folders': []})
        listing = engine.call('list', {'path': remote + '/backup'})
        assert any(e['name'] == 'sample.txt' for e in listing['entries'])
        report['checks'].update(folder_backup_create_update=True, backup_local_delete_does_not_delete_cloud=True)
        engine.call('delete', {'path': remote + '/moved.bin'})
        listing = engine.call('list', {'path': remote})
        assert {e['name'] for e in listing['entries']} == {'folder', 'backup'}
        report['checks']['trash_delete'] = True
    except Exception as error:
        failure = str(error)
        report['failed'] = failure
        report['network'] = engine.call('status').get('network', [])
    finally:
        # Never recursively remove a directory while a real cloud filesystem
        # is mounted beneath it, even when validation raises an exception.
        if os.path.ismount(root / 'mount'):
            engine.call('unmount', timeout=120)
        engine.call('save_backups', {'id': profile['id'], 'folders': []})
        try:
            if created:
                engine.call('delete', {'path': remote}, timeout=120)
                report['checks']['owned_remote_folder_trashed'] = True
        finally:
            engine.close()
        if not os.path.ismount(root / 'mount'):
            shutil.rmtree(root)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps(report, ensure_ascii=False))
    if failure:
        raise SystemExit(1)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--engine', type=Path, required=True)
    parser.add_argument('--peer', type=Path, required=True)
    parser.add_argument('--client-file', type=Path, required=True)
    parser.add_argument('--google-state', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--bytes', type=int, default=8 * 1024 * 1024 + 123)
    parser.add_argument('--mount', action='store_true')
    run(parser.parse_args())
