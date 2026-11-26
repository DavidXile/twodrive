#!/usr/bin/env python3
"""Signed test process requests over an already paired loopback WebDAV gateway.

No shell/OS path is sent to the remote host. Private keys stay in --state; a
child client uses a fresh private identity, separate from the control gateway.
"""
import argparse
import base64
import json
import re
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path


class Control:
    def __init__(self, binary, state, url, queue, device):
        if not re.fullmatch(r'td-control-[0-9a-f]{32}', queue):
            raise ValueError('Unexpected control directory')
        parsed = urllib.parse.urlsplit(url)
        if parsed.scheme != 'http' or parsed.hostname not in ('127.0.0.1', '::1') or parsed.username or parsed.password:
            raise ValueError('Control gateway must be HTTP on loopback without URL credentials')
        self.binary, self.state, self.device = str(binary), Path(state), device
        self.url = url.rstrip('/') + '/' + queue + '/'
        credentials = json.loads((self.state / 'credentials.json').read_text())
        self.auth = 'Basic ' + base64.b64encode((credentials['username'] + ':' + credentials['password']).encode()).decode()
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        self.owner = subprocess.check_output([self.binary, '--state', str(self.state), 'device-id'], text=True).strip()
        self.session = None

    def request(self, name, method='GET', body=None):
        req = urllib.request.Request(self.url + name, data=body, method=method, headers={'Authorization': self.auth})
        with self.opener.open(req, timeout=60) as response:
            data = response.read(256 * 1024 + 1)
            if len(data) > 256 * 1024:
                raise ValueError('Oversized response')
            return data

    def response(self):
        raw = self.request('response.json')
        with tempfile.TemporaryDirectory(prefix='td-control-response-') as directory:
            signed, plain = Path(directory) / 'signed.json', Path(directory) / 'verified.json'
            signed.write_bytes(raw)
            subprocess.run([self.binary, '--state', str(self.state), 'test-verify', '--input', str(signed), '--device', self.device, '--out', str(plain)], check=True, capture_output=True)
            result = json.loads(plain.read_text())
        if result['owner'] != self.owner:
            raise ValueError('Response has another control owner')
        if self.session is not None and result['session'] != self.session:
            raise ValueError('Control session changed')
        self.session = result['session']
        return result

    def command(self, action):
        previous = self.response()
        seq = previous['seq'] + 1
        payload = {'version': 1, 'session': self.session, 'seq': seq, 'expires': int(time.time()) + 120, 'command': action}
        with tempfile.TemporaryDirectory(prefix='td-control-request-') as directory:
            source, signed = Path(directory) / 'input.json', Path(directory) / 'signed.json'
            source.write_text(json.dumps(payload))
            subprocess.run([self.binary, '--state', str(self.state), 'test-sign', '--input', str(source), '--out', str(signed)], check=True, capture_output=True)
            signed_bytes = signed.read_bytes()
            request_signature = json.loads(signed_bytes)['signature']
            self.request('command.json', 'PUT', signed_bytes)
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            result = self.response()
            if result['seq'] == seq:
                if result['data'].get('request_signature') != request_signature:
                    raise ValueError('Signed response belongs to another request')
                return result
            if result['seq'] > seq:
                raise ValueError('Another controller advanced the command sequence')
            time.sleep(0.5)
        raise TimeoutError('Agent response deadline; transport remains available for retry')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--state', type=Path, required=True)
    parser.add_argument('--url', required=True)
    parser.add_argument('--queue', required=True)
    parser.add_argument('--device', required=True)
    parser.add_argument('--output', type=Path)
    actions = parser.add_subparsers(dest='action', required=True)
    actions.add_parser('ready')
    for action in ('snapshot', 'stop', 'finish'):
        actions.add_parser(action)
    start = actions.add_parser('start')
    start.add_argument('--client', required=True, help='Fresh test client public device identity')
    start.add_argument('--route', choices=('auto', 'direct', 'system'), default='auto')
    start.add_argument('--missing-interface', action='store_true')
    start.add_argument('--no-relay', action='store_true')
    start.add_argument('--relay-only', action='store_true')
    start.add_argument('--relay', action='append', default=[])
    start.add_argument('--candidate-sha256', help='SHA256 of candidate.bin already uploaded to the queue')
    args = parser.parse_args()
    control = Control(args.binary, args.state, args.url, args.queue, args.device)
    if args.action == 'ready':
        result = control.response()
    elif args.action == 'start':
        result = control.command({'action': 'start', 'client': args.client, 'candidate_sha256': args.candidate_sha256, 'route': args.route, 'missing_interface': args.missing_interface, 'no_relay': args.no_relay, 'relay_only': args.relay_only, 'relays': args.relay})
    else:
        result = control.command({'action': args.action})
    text = json.dumps(result, indent=2) + '\n'
    if args.output:
        args.output.write_text(text)
    else:
        print(text, end='')


if __name__ == '__main__':
    main()
