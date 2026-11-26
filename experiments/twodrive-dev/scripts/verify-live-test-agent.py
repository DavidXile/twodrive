#!/usr/bin/env python3
"""Resume real two-device validation after one Windows test-agent bootstrap.

The original gateway stays up. Each child/client has disposable independent
state. A failed stage retains the signed agent diagnostics for a remote retry;
only a successful complete run sends finish. No credentials enter the report.
"""
import argparse
import base64
import hashlib
import importlib.util
import ipaddress
import json
import signal
import socket
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

SPEC = importlib.util.spec_from_file_location('control', Path(__file__).with_name('control-test-agent.py'))
CONTROL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CONTROL)


def save(path, value):
    temporary = path.with_suffix('.tmp')
    temporary.write_text(json.dumps(value, indent=2) + '\n')
    temporary.replace(path)


def port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


class Gateway:
    def __init__(self, binary, state, directory, missing=False):
        self.base = f'http://127.0.0.1:{port()}/'
        self.log_path = directory / 'client.log'
        self.log = self.log_path.open('w')
        args = [str(binary), '--state', str(state), 'connect', '--listen', self.base[7:-1]]
        if missing:
            args += ['--network-interface', 'twodrive-test-missing-interface']
        self.process = subprocess.Popen(args, stdout=self.log, stderr=self.log)
        self.state = state
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def request(self, path='', method='OPTIONS', data=None, headers=None):
        c = json.loads((self.state / 'credentials.json').read_text())
        auth = 'Basic ' + base64.b64encode((c['username'] + ':' + c['password']).encode()).decode()
        req = urllib.request.Request(self.base + path, data=data, method=method, headers={'Authorization': auth, **(headers or {})})
        with self.opener.open(req, timeout=180) as response:
            return response.status, response.read(), dict(response.headers)

    def ready(self):
        deadline = time.monotonic() + 100
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError('Test client exited: ' + self.log_path.read_text()[-4000:])
            try:
                if self.request()[0] == 200:
                    return
            except (OSError, ValueError):
                pass
            time.sleep(0.5)
        raise TimeoutError('Test client gateway startup')

    def close(self):
        if self.process.poll() is None:
            self.process.send_signal(signal.SIGINT)
            try:
                self.process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
        self.log.close()


def probe(gateway):
    name = 'td-probe-' + uuid.uuid4().hex
    moved = name + '-moved'
    data = (bytes(range(251)) * 8356)[:2 * 1024 * 1024]
    expected = hashlib.sha256(data).hexdigest()
    result = {'bytes': len(data), 'sha256': expected}
    try:
        start = time.monotonic()
        assert gateway.request(name, 'PUT', data, {'If-None-Match': '*'})[0] == 201
        result['put_seconds'] = round(time.monotonic() - start, 3)
        start = time.monotonic()
        status, downloaded, _ = gateway.request(name, 'GET')
        assert status == 200 and len(downloaded) == len(data)
        assert hashlib.sha256(downloaded).hexdigest() == expected
        result['get_seconds'] = round(time.monotonic() - start, 3)
        assert gateway.request(name, 'MOVE', headers={'Destination': gateway.base + moved, 'Overwrite': 'F'})[0] == 201
        status, ranged, headers = gateway.request(moved, 'GET', headers={'Range': 'bytes=131072-393215'})
        headers = {k.lower(): v for k, v in headers.items()}
        assert status == 206 and ranged == data[131072:393216]
        assert headers['content-range'] == f'bytes 131072-393215/{len(data)}'
        assert gateway.request(moved, 'DELETE')[0] == 204
        try:
            gateway.request(moved, 'GET')
            raise AssertionError('Deleted probe still exists')
        except urllib.error.HTTPError as error:
            assert error.code == 404
        result.update(full_hash=True, move=True, range_bytes=len(ranged), delete_404=True)
        return result
    finally:
        for path in (name, moved):
            try:
                gateway.request(path, 'DELETE')
            except OSError:
                pass


def route_stamp():
    routes = subprocess.check_output(['ip', '-j', 'route', 'show', 'table', 'all'], text=True)
    rules = subprocess.check_output(['ip', '-j', 'rule', 'show'], text=True)
    return hashlib.sha256((routes + rules).encode()).digest()


class Sampler:
    def __init__(self, pid):
        self.pid = pid
        self.stop = threading.Event()
        self.stats = dict(samples=0, public_tcp_samples=0, physical_bound_tcp_samples=0, nonphysical_tcp_samples=0, route_unchanged=True)
        self.initial = route_stamp()
        addresses = json.loads(subprocess.check_output(['ip', '-j', 'address'], text=True))
        self.stats['proxy_fake_ip_present'] = any(ipaddress.ip_address(a['local']) in ipaddress.ip_network('198.18.0.0/15') for interface in addresses for a in interface['addr_info'] if a['family'] == 'inet')
        self.physical = {interface['ifname'] for interface in addresses if (Path('/sys/class/net') / interface['ifname'] / 'device').exists()}
        self.thread = threading.Thread(target=self.run, daemon=True)
        self.thread.start()

    def run(self):
        try:
            while not self.stop.is_set():
                self.stats['samples'] += 1
                self.stats['route_unchanged'] &= route_stamp() == self.initial
                sockets = subprocess.check_output(['ss', '-ntpe'], text=True)
                for line in sockets.splitlines():
                    if f'pid={self.pid},' not in line:
                        continue
                    fields = line.split()
                    if fields[0] != 'ESTAB' or not fields[4].endswith(':443'):
                        continue
                    remote = fields[4].rsplit(':', 1)[0].strip('[]')
                    if ipaddress.ip_address(remote).is_loopback:
                        continue
                    self.stats['public_tcp_samples'] += 1
                    bound = fields[3].rsplit(':', 1)[0].strip('[]').rsplit('%', 1)
                    if len(bound) == 2 and bound[1] in self.physical:
                        self.stats['physical_bound_tcp_samples'] += 1
                    else:
                        self.stats['nonphysical_tcp_samples'] += 1
                self.stop.wait(1)
        except Exception as error:
            self.stats['error_type'] = type(error).__name__

    def close(self):
        self.stop.set()
        self.thread.join(timeout=10)
        if self.thread.is_alive():
            raise RuntimeError('Owned Linux sampler did not stop')
        return self.stats


def run(args):
    controller = CONTROL.Control(args.binary, args.state, args.url, args.queue, args.device)
    result = {'status': 'waiting', 'date': time.strftime('%Y-%m-%d'), 'stages': [], 'network_context': args.network_context, 'custom_relays': args.relay, 'candidate_sha256': args.candidate_sha256}
    save(args.output, result)
    before = {p.name: hashlib.sha256(p.read_bytes()).digest() for p in args.state.iterdir() if p.is_file()}
    deadline = time.monotonic() + args.wait_ready
    while True:
        try:
            ready = controller.response()
            assert ready['status'] not in ('finished', 'expired_or_revoked'), 'Agent lease has ended'
            if ready['seq'] == 0 and ready['status'] == 'ready':
                assert ready['data']['agent_sha256'] == args.windows_sha256, 'Unexpected Windows controller executable'
            else:
                assert args.resume, 'Agent already has work; inspect its snapshot and use --resume for an intentional retry'
            result['ready'] = ready
            print('Windows signed agent ready; starting real validation', flush=True)
            break
        except urllib.error.HTTPError as error:
            if error.code != 404 or time.monotonic() >= deadline:
                raise
            time.sleep(2)
    result['status'] = 'running'
    save(args.output, result)
    stages = [('both_auto', False, False), ('linux_fallback', False, True), ('windows_fallback', True, False)]
    if args.stage != 'all':
        stages = [s for s in stages if s[0] == args.stage]
    try:
        for name, windows_missing, linux_missing in stages:
            stage = {'name': name}
            result['stages'].append(stage)
            save(args.output, result)
            print('Starting ' + name, flush=True)
            with tempfile.TemporaryDirectory(prefix='td-live-client-', dir=args.output.parent) as tmp:
                root = Path(tmp)
                state = root / 'state'
                state.mkdir(mode=0o700)
                client = subprocess.check_output([str(args.binary), '--state', str(state), 'device-id'], text=True).strip()
                assert client != controller.owner and client != args.device
                started = controller.command(dict(action='start', client=client, candidate_sha256=args.candidate_sha256, route='auto', missing_interface=windows_missing, no_relay=False, relay_only=False, relays=args.relay))
                stage['start'] = started
                assert started['status'] == 'ok', 'Windows child start rejected'
                limit = time.monotonic() + 110
                while True:
                    snapshot = controller.command({'action': 'snapshot'})
                    stage['windows'] = snapshot
                    save(args.output, result)
                    assert snapshot['status'] == 'ok', 'Windows snapshot failed'
                    info = snapshot['data']
                    assert not info.get('exited'), 'Windows child exited; signed stderr retained in report'
                    if info.get('ready'):
                        assert info['executable_sha256'] == (args.candidate_sha256 or args.windows_sha256), 'Unexpected Windows child executable'
                        break
                    if time.monotonic() >= limit:
                        raise TimeoutError('Windows child startup; signed diagnostics retained')
                    time.sleep(1)
                (state / 'remote.json').write_text(json.dumps(info['address']))
                gateway, sampler = None, None
                try:
                    gateway = Gateway(args.binary, state, root, linux_missing)
                    sampler = Sampler(gateway.process.pid)
                    gateway.ready()
                    stage['data'] = probe(gateway)
                finally:
                    if gateway:
                        gateway.close()
                        stage['linux_log'] = gateway.log_path.read_text()[-16384:]
                    if sampler:
                        stage['linux_network'] = sampler.close()
                    save(args.output, result)
                stopped = controller.command({'action': 'stop'})
                stage['stop'] = stopped
                assert stopped['status'] == 'ok' and stopped['data']['temporary_child_removed']
                info = stopped['data']['child']
                network = info['network']
                stage['windows_final'] = info
                assert network and network['samples'] > 0
                assert network['route_unchanged'] and network['proxy_fake_ip_present'] and 'error_type' not in network
                if windows_missing:
                    assert 'route=system/proxy fallback' in info['stderr']
                    # A local HTTP CONNECT proxy owns its outbound 443 socket;
                    # the child can have zero public-443 samples in this mode.
                    assert network['public_tcp_samples'] > 0 or 'proxy=windows-user-http' in info['stderr'] or 'proxy=environment' in info['stderr']
                else:
                    assert 'route=physical' in info['stderr'] and 'route=system/proxy fallback' not in info['stderr']
                    assert network['nonphysical_tcp_samples'] == 0
                    assert network['public_tcp_samples'] > 0
                linux = stage['linux_network']
                assert linux['route_unchanged'] and linux['proxy_fake_ip_present'] and 'error_type' not in linux
                if linux_missing:
                    assert 'route=system/proxy fallback' in stage['linux_log']
                    assert linux['public_tcp_samples'] > 0 or 'proxy=environment' in stage['linux_log']
                else:
                    assert 'route=physical' in stage['linux_log'] and 'route=system/proxy fallback' not in stage['linux_log']
                    assert linux['nonphysical_tcp_samples'] == 0
                    assert linux['public_tcp_samples'] > 0
                stage['passed'] = True
                save(args.output, result)
                print('Passed ' + name + ': 2 MiB hash, move, range and cleanup', flush=True)
        result['status'] = 'passed'
        if args.stage == 'all':
            result['finish'] = controller.command({'action': 'finish'})
            assert result['finish']['status'] == 'finished'
    except Exception as error:
        result['status'] = 'failed'
        result['error'] = str(error)
        try:
            result['failure_snapshot'] = controller.command({'action': 'snapshot'})
            result['failure_stop'] = controller.command({'action': 'stop'})
        except Exception as cleanup_error:
            result['cleanup_error'] = str(cleanup_error)
        raise
    finally:
        after = {p.name: hashlib.sha256(p.read_bytes()).digest() for p in args.state.iterdir() if p.is_file()}
        result['original_client_state_preserved'] = before == after
        save(args.output, result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--state', type=Path, required=True)
    parser.add_argument('--url', default='http://127.0.0.1:4919/')
    parser.add_argument('--queue', required=True)
    parser.add_argument('--device', required=True)
    parser.add_argument('--windows-sha256', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--network-context', required=True, help='Actual current networks; do not assume independent NAT')
    parser.add_argument('--wait-ready', type=int, default=180)
    parser.add_argument('--stage', choices=('all', 'both_auto', 'linux_fallback', 'windows_fallback'), default='all')
    parser.add_argument('--resume', action='store_true', help='Intentionally restart test stages using an already active signed session')
    parser.add_argument('--relay', action='append', default=[], help='Explicit child relay for diagnostics; recorded separately from default discovery')
    parser.add_argument('--candidate-sha256', help='SHA256 of candidate.bin already delivered to the control queue')
    args = parser.parse_args()
    args.binary = args.binary.resolve(strict=True)
    args.state = args.state.resolve(strict=True)
    args.output = args.output.resolve()
    try:
        run(args)
    except Exception as error:
        if args.output.exists():
            result = json.loads(args.output.read_text())
            if result['status'] == 'waiting':
                result.update(status='not_started', error=str(error))
                save(args.output, result)
        raise


if __name__ == '__main__':
    main()
