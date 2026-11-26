#!/usr/bin/env python3
"""Real Windows desktop IPC with generated files; no registration/cloud/content import."""
import argparse
import importlib.util
import json
from pathlib import Path
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--engine', required=True)
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location('desktop_fixture', Path(__file__).with_name('test-dav-desktop.py'))
    fixture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture)
    with tempfile.TemporaryDirectory(prefix='twodrive-local-ipc-') as directory:
        root = Path(directory)
        source = root / 'generated-中文.txt'
        source.write_bytes(b'generated metadata-only IPC fixture')
        engine = fixture.Engine(str(Path(args.engine).resolve()), str(Path(args.engine).resolve()), root)
        try:
            engine.call('subscribe', {'operations': True})
            status = engine.call('status')
            assert status['local_files']['schema_version'] == 1
            assert status['local_files']['metadata_preflight']
            assert not status['local_files']['native_mount']
            assert not status['capabilities']['mount']
            result = engine.call('local_preflight', {'schema_version': 1, 'source_path': str(source), 'source_kind': 'ordinary_directory'})
            assert result['decision'] == 'full_file_needs_approved_plan'
            assert result['approval_required'] and not result['content_read']
            assert not result['hydration_requested'] and not result['import_performed']
            assert str(source) not in json.dumps(result)
            engine.call('local_preflight', {'schema_version': 2}, failure=True)
            for kind, expected in [('private_cache','unsupported_cache'),('virtual_mount','unsupported_virtual_mount'),('unknown','unknown_provenance')]:
                result = engine.call('local_preflight', {'schema_version': 1, 'source_path': str(root / 'not-present'), 'source_kind': kind})
                assert result['decision'] == expected
            assert source.read_bytes() == b'generated metadata-only IPC fixture'
            assert engine.operation_events == 0, 'metadata preflight must not advertise sync work'
            print(json.dumps({'actual_desktop_ipc_preflight': 'passed', 'cloud_or_registration': False, 'source_bytes_preserved': True}))
        finally:
            engine.close()


if __name__ == '__main__':
    main()
