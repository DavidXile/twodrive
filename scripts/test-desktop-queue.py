#!/usr/bin/env python3
"""Deterministic IPC backpressure/cancellation check with a gated loopback DAV.

No cloud account, installed service or private user state is used.
"""
import argparse
import importlib.util
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import queue
import tempfile
import threading
import time

spec = importlib.util.spec_from_file_location('desktop_check', Path(__file__).with_name('test-dav-desktop.py'))
desktop = importlib.util.module_from_spec(spec)
spec.loader.exec_module(desktop)


def run(args):
    started, release = threading.Event(), threading.Event()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_PROPFIND(self):
            started.set()
            assert release.wait(30), 'test gate was not released'
            body = b'''<?xml version="1.0"?><d:multistatus xmlns:d="DAV:">
<d:response><d:href>/</d:href><d:propstat><d:prop>
<d:resourcetype><d:collection/></d:resourcetype><d:getetag>"root"</d:getetag>
</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>'''
            self.send_response(207)
            self.send_header('Content-Type', 'application/xml')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    with tempfile.TemporaryDirectory(prefix='td-queue-check-') as tmp:
        engine = desktop.Engine(args.engine, args.peer, Path(tmp))
        try:
            engine.call('save_profile', {
                'id': 'slow', 'kind': 'dav', 'name': 'Slow test connection',
                'url': f'http://127.0.0.1:{server.server_port}/',
                'username': 'Test account', 'network_route': 'system',
            })

            assert engine.operation_events == 0, 'old clients must not receive unsolicited operation events'
            assert engine.call('subscribe', {'operations': True})['operations'] is True

            def send(request_id, op, args=None):
                engine.process.stdin.write(json.dumps({'id': request_id, 'op': op, 'args': args or {}}) + '\n')
                engine.process.stdin.flush()

            send(100, 'connect', {'id': 'slow'})
            assert started.wait(10), 'connect did not reach the deterministic gate'
            # Fill all 32 slots while the worker waits at the server. A queued
            # mutation must be skipped when cancelled before it can execute.
            send(101, 'save_profile', {'id': 'cancelled', 'kind': 'dav', 'name': 'Must not persist',
                                      'url': 'https://example.invalid/', 'network_route': 'system'})
            for request_id in range(102, 133):
                send(request_id, 'status')
            send(133, 'status')
            send(134, 'cancel', {'id': 101})
            responses, events = {}, []

            def receive_until(ids):
                deadline = time.monotonic() + 20
                while not ids.issubset(responses):
                    try:
                        message = engine.events.get(timeout=max(.01, deadline - time.monotonic()))
                    except queue.Empty:
                        raise AssertionError('IPC replies did not complete within test budget') from None
                    if message.get('event'):
                        events.append(message)
                    else:
                        responses[message['id']] = message

            receive_until({133, 134})
            assert responses[133]['ok'] is False and 'queue is full' in responses[133]['error']
            assert responses[134]['ok'] is True, 'cancellation blocked behind full queue'
            assert not release.is_set()
            release.set()
            receive_until(set(range(100, 135)))
            assert responses[100]['ok'], responses[100].get('error')
            assert not responses[101]['ok'] and '取消' in responses[101]['error']
            assert all(responses[i]['ok'] for i in range(102, 133))
            completed = [e for e in events if e.get('event') == 'operation' and
                         e['id'] == 100 and not e['running']]
            assert len(completed) == 1
            assert completed[0]['status']['connected'] is True
            assert completed[0]['status']['account_name'] == 'Test account'
            engine.counter = 200
            public = engine.call('profiles')
            assert [p['id'] for p in public['profiles']] == ['slow']
            assert public['profiles'][0]['account_name'] == 'Test account'
            engine.call('disconnect')
            assert not engine.call('status')['connected']
            print(json.dumps({'bounded_queue': True, 'cancel_bypasses_full_queue': True,
                              'cancelled_mutation_skipped': True, 'completion_state_event': True,
                              'actual_dav_username': True, 'queue_recovers': True, 'notifications_opt_in': True}))
        finally:
            release.set()
            engine.close()
            server.shutdown()
            server.server_close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--engine', required=True)
    parser.add_argument('--peer', required=True)
    run(parser.parse_args())
