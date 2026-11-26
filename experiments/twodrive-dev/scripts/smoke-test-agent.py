#!/usr/bin/env python3
"""Real QUIC/WebDAV control transport, independent child and autonomous cleanup."""
import argparse
import base64
import hashlib
import importlib.util
import json
import socket
import subprocess
import tempfile
import time
import urllib.request
import uuid
from pathlib import Path

SPEC = importlib.util.spec_from_file_location('control_test_agent', Path(__file__).with_name('control-test-agent.py'))
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
LIVE_SPEC = importlib.util.spec_from_file_location('live_validation', Path(__file__).with_name('verify-live-test-agent.py'))
LIVE = importlib.util.module_from_spec(LIVE_SPEC)
LIVE_SPEC.loader.exec_module(LIVE)


def wait(check, processes, seconds=40):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if any(p.poll() is not None for p in processes):
            raise RuntimeError('Owned process exited unexpectedly')
        try:
            value = check()
            if value:
                return value
        except (OSError, ValueError):
            pass
        time.sleep(0.1)
    raise TimeoutError('Control smoke deadline')


def port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with tempfile.TemporaryDirectory(prefix='td-control-smoke-') as directory:
        sandbox = Path(directory)
        share = sandbox / 'share'
        share.mkdir()
        queue = share / ('td-control-' + uuid.uuid4().hex)
        queue.mkdir()
        ticket = sandbox / 'invite.json'
        server, owner, child = [sandbox / name for name in ('server', 'owner', 'child-client')]
        child.mkdir(mode=0o700)
        child_runtime = sandbox / 'temp'
        child_runtime.mkdir()
        p_control, p_child = port(), port()
        processes, logs = [], []

        def launch(name, command, environment=None):
            log = (sandbox / (name + '.log')).open('w+')
            logs.append(log)
            proc = subprocess.Popen(command, stdout=log, stderr=log, env=environment)
            processes.append(proc)
            return proc

        def request(base, state, path='', method='OPTIONS', data=None):
            auth = json.loads((state / 'credentials.json').read_text())
            header = 'Basic ' + base64.b64encode((auth['username'] + ':' + auth['password']).encode()).decode()
            req = urllib.request.Request(base + path, data=data, method=method, headers={'Authorization': header})
            with opener.open(req, timeout=15) as response:
                return response.read() if method != 'OPTIONS' else response.status

        try:
            stable = launch('original', [str(binary), '--state', str(server), 'serve', '--root', str(share), '--write', '--invite-file', str(ticket), '--no-relay', '--network-route', 'system'])
            wait(lambda: ticket.exists() and json.loads(ticket.read_text()), processes)
            gateway = launch('control-gateway', [str(binary), '--state', str(owner), 'connect', '--ticket-file', str(ticket), '--listen', f'127.0.0.1:{p_control}', '--no-relay', '--network-route', 'system'])
            base = f'http://127.0.0.1:{p_control}/'
            wait(lambda: request(base, owner), processes)
            before = {name: hashlib.sha256((server / name).read_bytes()).digest() for name in ('identity.json', 'control.json')}
            ids = [subprocess.check_output([str(binary), '--state', str(state), 'device-id'], text=True).strip() for state in (server, owner, child)]
            import os
            environment = dict(os.environ, TMPDIR=str(child_runtime), TEMP=str(child_runtime), TMP=str(child_runtime))
            agent = launch('agent', [str(binary), '--state', str(server), 'test-agent', '--queue', str(queue), '--owner', ids[1], '--lifetime', '120'], environment)
            controller = MODULE.Control(binary, owner, base, queue.name, ids[0])
            wait(controller.response, processes)
            start = {'action': 'start', 'client': ids[2], 'candidate_sha256': None, 'route': 'system', 'missing_interface': False, 'no_relay': True, 'relay_only': False, 'relays': []}
            assert controller.command(start)['status'] == 'ok'
            def ready_child():
                response = controller.command({'action': 'snapshot'})
                return response['data'] if response['data'].get('ready') else None
            info = wait(ready_child, processes)
            (child / 'remote.json').write_text(json.dumps(info['address']))
            client = launch('child-gateway', [str(binary), '--state', str(child), 'connect', '--listen', f'127.0.0.1:{p_child}', '--network-route', 'system', '--no-relay'])
            child_base = f'http://127.0.0.1:{p_child}/'
            wait(lambda: request(child_base, child), processes)
            payload = (bytes(range(251)) * 4178)[:1024 * 1024]
            request(child_base, child, 'payload.bin', 'PUT', payload)
            assert hashlib.sha256(request(child_base, child, 'payload.bin', 'GET')).digest() == hashlib.sha256(payload).digest()
            request(child_base, child, 'payload.bin', 'DELETE')
            # Exercise the two-device verifier's data probe through the real
            # independently authenticated temporary child.
            live_gateway = object.__new__(LIVE.Gateway)
            live_gateway.base, live_gateway.state, live_gateway.opener = child_base, child, opener
            validation = LIVE.probe(live_gateway)
            assert validation['bytes'] == 2 * 1024 * 1024 and validation['delete_404']
            # Stop the test server while the independent original transport lives.
            client.terminate()
            client.wait(timeout=10)
            processes.remove(client)
            assert controller.command({'action': 'stop'})['data']['temporary_child_removed']
            assert stable.poll() is None and gateway.poll() is None
            assert request(base, owner) == 200
            done = controller.command({'action': 'finish'})
            assert done['status'] == 'finished'
            agent.wait(timeout=10)
            processes.remove(agent)
            assert agent.returncode == 0
            assert not list(child_runtime.iterdir())
            for name, digest in before.items():
                assert hashlib.sha256((server / name).read_bytes()).digest() == digest
            print('PASS: signed jobs through real encrypted TwoDrive, separate temporary child identity, 1 MiB roundtrip plus live 2 MiB hash/move/range/delete probe, original server stays available, child/agent cleanup and identity preservation')
        finally:
            for process in reversed(processes):
                process.terminate()
            for process in reversed(processes):
                try:
                    process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
            for log in logs:
                log.close()


if __name__ == '__main__':
    main()
