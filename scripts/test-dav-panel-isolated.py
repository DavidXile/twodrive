#!/usr/bin/env python3
"""Actual GNOME compositor/GTK panel test in private displays and session buses.

Never activates the user's desktop, moves its pointer, or uses cloud credentials.
Requires GNOME 45+, Zorin/Ubuntu AppIndicators, and Xvfb for the X11 test.
"""
import argparse
import ast
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time

REPO = Path(__file__).resolve().parent.parent
UUID = 'twodrive-tray-anchor@twodrive'


def call(method, *args, dest='org.gnome.Shell', path='/app/twodrive/TrayAnchor'):
    r = subprocess.run(['gdbus', 'call', '--session', '--dest', dest, '--object-path',
                        path, '--method', method, *map(str, args)],
                       capture_output=True, text=True, timeout=4, check=True)
    return ast.literal_eval(r.stdout.replace('true', 'True').replace('false', 'False'))


def stop(child):
    if child and child.poll() is None:
        os.killpg(child.pid, signal.SIGTERM)
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait(timeout=5)


def inside(args):
    root = args.root
    assert str(root).startswith('/tmp/td-panel-isolated-')
    assert os.environ['XDG_RUNTIME_DIR'] == str(root/'runtime')
    indicators = ['zorin-appindicator@zorinos.com', 'ubuntu-appindicators@ubuntu.com',
                  'appindicatorsupport@rgcjonas.gmail.com']
    host = next(i for i in indicators if (Path('/usr/share/gnome-shell/extensions')/i).exists())
    subprocess.run(['gsettings', 'set', 'org.gnome.shell', 'enabled-extensions', repr([host, UUID])], check=True)
    shell = display = app = None
    try:
        shell_env = dict(os.environ)
        if args.backend == 'x11':
            with (root/'display.log').open('w') as log:
                display = subprocess.Popen([args.xvfb, '-displayfd', '1', '-screen', '0',
                    '1440x900x24', '-nolisten', 'tcp'], stdout=subprocess.PIPE, stderr=log,
                    text=True, start_new_session=True)
            number = display.stdout.readline().strip()
            assert number.isdigit()
            shell_env['DISPLAY'] = ':'+number
            command = ['gnome-shell', '--x11', '--display', ':'+number, '--mode', 'user', '--sm-disable']
        else:
            shell_env['WAYLAND_DISPLAY'] = 'td-panel-test'
            command = ['gnome-shell', '--headless', '--wayland', '--no-x11',
                       '--virtual-monitor', '1440x900', '--wayland-display', 'td-panel-test', '--mode', 'user']
        with (root/'shell.log').open('w') as log:
            shell = subprocess.Popen(command, env=shell_env, stdout=log, stderr=log,
                                     start_new_session=True)
        deadline = time.monotonic()+40
        while True:
            assert shell.poll() is None, 'isolated compositor exited; inspect shell.log'
            try:
                call('app.twodrive.TrayAnchor1.GetAnchor', 0)
                break
            except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
                assert time.monotonic()<deadline, 'isolated helper did not load'
                time.sleep(.2)
        env = dict(shell_env, GDK_BACKEND=args.backend, GDK_SCALE='1')
        with (root/'client.log').open('w') as log:
            app = subprocess.Popen([str(args.bundle/'twodrive_dav'), '--background', '--state',
                str(root/'state'), '--engine', str(args.engine), '--peer-bin', str(args.peer)],
                env=env, stdout=log, stderr=log, start_new_session=True)
        while True:
            assert app.poll() is None, 'isolated client exited; inspect client.log'
            anchor = call('app.twodrive.TrayAnchor1.GetAnchor', app.pid)
            if anchor[0]: break
            assert time.monotonic()<deadline, 'actual tray actor not found'
            time.sleep(.2)
        if args.backend == 'x11':
            r = subprocess.run([sys.executable, str(REPO/'scripts/test-dav-tray-x11.py'),
                '--pid', str(app.pid), '--virtual-input'], env=env, capture_output=True, text=True, timeout=20)
            if r.returncode:
                raise RuntimeError(r.stderr[-2200:])
            report = json.loads(r.stdout)
            assert report['icon_geometry_verified']
        else:
            _, ix, iy, iw, ih, ax, ay, aw, ah = anchor
            width, height = min(390, aw-16), min(570, ah-16)
            expected = (max(ax+8, min(ix+iw//2-width//2, ax+aw-width-8)),
                        ay+8 if iy+ih//2 < ay else ay+ah-height-8, width, height)
            assert not call('app.twodrive.TrayAnchor1.GetPanel', app.pid)[0], 'background startup mapped a window'
            # A different process cannot ask the bridge to move this window.
            assert not call('app.twodrive.TrayAnchor1.PositionPanel', app.pid)[0]
            for _ in range(3):
                time.sleep(.4)
                call('org.kde.StatusNotifierItem.Activate', 16, 16,
                     dest=f'org.kde.StatusNotifierItem-{app.pid}-1', path='/StatusNotifierItem')
                time.sleep(.8)
                actual = call('app.twodrive.TrayAnchor1.GetPanel', app.pid)
                assert actual[0] and actual[1:] == expected, (actual, expected)
                call('org.kde.StatusNotifierItem.Activate', 16, 16,
                     dest=f'org.kde.StatusNotifierItem-{app.pid}-1', path='/StatusNotifierItem')
                time.sleep(.4)
                assert not call('app.twodrive.TrayAnchor1.GetPanel', app.pid)[0]
            report = dict(icon=anchor[1:5], workarea=anchor[5:], panel=expected,
                          repeat_open_close=3, actor_geometry_verified=True,
                          wrong_process_position_rejected=True, background_start_hidden=True)
        shell_version = subprocess.check_output(['gnome-shell', '--version'], text=True).strip()
        report.update(environment=f'isolated {shell_version} / native GTK {args.backend}',
                      foreground_desktop_input=False)
        args.output.write_text(json.dumps(report, indent=2)+'\n')
        print(json.dumps(report, indent=2))
    finally:
        stop(app)
        stop(shell)
        stop(display)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--backend', choices=['x11', 'wayland'], required=True)
    parser.add_argument('--bundle', type=Path, required=True)
    parser.add_argument('--engine', type=Path, required=True)
    parser.add_argument('--peer', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--xvfb', default='Xvfb')
    parser.add_argument('--root', type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    for name in ['bundle', 'engine', 'peer', 'output']:
        setattr(args, name, getattr(args, name).resolve())
    if args.root:
        inside(args)
    else:
        with tempfile.TemporaryDirectory(prefix='td-panel-isolated-') as tmp:
            root = Path(tmp)
            (root/'runtime').mkdir(mode=0o700)
            shutil.copytree(REPO/'packaging/gnome-tray-anchor', root/'data/gnome-shell/extensions'/UUID)
            env = dict(os.environ, XDG_CONFIG_HOME=str(root/'config'), XDG_DATA_HOME=str(root/'data'),
                       XDG_RUNTIME_DIR=str(root/'runtime'), XDG_CACHE_HOME=str(root/'cache'),
                       XDG_CURRENT_DESKTOP='GNOME', XDG_SESSION_TYPE=args.backend,
                       LIBGL_ALWAYS_SOFTWARE='1')
            for key in ['DISPLAY', 'WAYLAND_DISPLAY', 'DBUS_SESSION_BUS_ADDRESS', 'GDK_BACKEND']:
                env.pop(key, None)
            command = ['dbus-run-session', '--', sys.executable, str(Path(__file__).resolve()),
                '--backend', args.backend, '--bundle', str(args.bundle), '--engine', str(args.engine),
                '--peer', str(args.peer), '--output', str(args.output), '--xvfb', args.xvfb, '--root', str(root)]
            subprocess.run(command, env=env, check=True, timeout=100)
