#!/usr/bin/env python3
"""Record short Nautilus responsiveness and thread-state diagnostics.

The database contains process names, wait-channel names, timings, and counts;
it intentionally does not collect paths, command lines, journal messages, or
file contents. Rows older than 48 hours are removed on every probe.
"""

import argparse
import collections
import json
import os
from pathlib import Path
import sqlite3
import stat
import subprocess
import time


RETENTION_SECONDS = 48 * 60 * 60
DESTINATION = "org.gnome.Nautilus"
OBJECT_PATH = "/org/gnome/Nautilus"


def process_ids(name):
    ids = []
    for entry in Path("/proc").iterdir():
        if not entry.name.isdecimal():
            continue
        try:
            if (entry / "comm").read_text().strip() == name:
                ids.append(int(entry.name))
        except (OSError, UnicodeError):
            continue
    return sorted(ids)


def thread_snapshot(pid):
    threads = []
    main = None
    for task in (Path("/proc") / str(pid) / "task").glob("[0-9]*"):
        state, wchan = read_thread_state(task)
        if state is None:
            continue
        threads.append((state, wchan))
        if task.name == str(pid):
            main = {"state": state, "wchan": wchan}
    counts = collections.Counter(threads)
    return {
        "pid": pid,
        "threads": len(threads),
        "main": main,
        "waits": [{"state": state, "wchan": wchan, "count": count}
                  for (state, wchan), count in sorted(counts.items())],
    }


def read_thread_state(task):
    try:
        # The comm field can contain spaces and parentheses.
        fields = (task / "stat").read_text().rsplit(") ", 1)[1].split()
        return fields[0], (task / "wchan").read_text().strip() or "unknown"
    except (OSError, IndexError):
        return None, None


def probe():
    started = time.monotonic()
    try:
        result = subprocess.run(
            ["gdbus", "call", "--session", "--timeout", "2",
             "--dest", DESTINATION, "--object-path", OBJECT_PATH,
             "--method", "org.freedesktop.DBus.Peer.Ping"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            timeout=3, check=False,
        )
        status = "ok" if result.returncode == 0 else "error"
    except subprocess.TimeoutExpired:
        status = "timeout"
    except OSError:
        status = "probe_unavailable"
    return status, round((time.monotonic() - started) * 1000)


def open_database(path):
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    parent_stat = path.parent.stat()
    if parent_stat.st_uid != os.getuid() or stat.S_IMODE(parent_stat.st_mode) & 0o077:
        raise PermissionError(f"diagnostic directory must be private: {path.parent}")
    if not path.exists():
        fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
        os.close(fd)
    database_stat = path.stat()
    if database_stat.st_uid != os.getuid() or stat.S_IMODE(database_stat.st_mode) & 0o077:
        raise PermissionError(f"diagnostic database must be private: {path}")
    connection = sqlite3.connect(path)
    connection.execute("PRAGMA auto_vacuum=FULL")
    connection.execute(
        "CREATE TABLE IF NOT EXISTS events "
        "(time INTEGER NOT NULL, kind TEXT NOT NULL, details TEXT NOT NULL)"
    )
    connection.execute("CREATE INDEX IF NOT EXISTS events_time ON events(time)")
    connection.commit()
    return connection


def record(connection, now, kind, details):
    connection.execute(
        "INSERT INTO events VALUES (?, ?, ?)",
        (now, kind, json.dumps(details, ensure_ascii=False, sort_keys=True)),
    )
    connection.commit()


def monitor(connection, interval, slow_ms, once=False):
    previous = None
    last_bad = 0
    last_heartbeat = 0
    while True:
        now = int(time.time())
        connection.execute("DELETE FROM events WHERE time < ?", (now - RETENTION_SECONDS,))
        connection.commit()
        nautilus = process_ids("nautilus")
        if nautilus:
            status, elapsed_ms = probe()
            kind = "slow" if status == "ok" and elapsed_ms >= slow_ms else status
        else:
            kind, elapsed_ms = "absent", None
        details = {"nautilus_pids": nautilus, "ping_ms": elapsed_ms}
        bad = kind in ("slow", "timeout", "error", "probe_unavailable")
        if bad and (kind != previous or now - last_bad >= 30):
            details["nautilus_threads"] = [thread_snapshot(pid) for pid in nautilus]
            details["daemon_threads"] = [thread_snapshot(pid) for pid in process_ids("twodrive-daemon")]
            record(connection, now, kind, details)
            last_bad = now
            last_heartbeat = now
        elif kind != previous:
            record(connection, now, "recovered" if previous in ("slow", "timeout", "error") and kind == "ok" else kind, details)
            last_heartbeat = now
        elif now - last_heartbeat >= 3600:
            record(connection, now, "heartbeat", details)
            last_heartbeat = now
        previous = kind
        if once:
            return
        time.sleep(interval)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database", type=Path,
                        default=Path.home() / ".local/state/twodrive/nautilus-watch/events.sqlite3")
    parser.add_argument("--interval", type=float, default=5.0)
    parser.add_argument("--slow-ms", type=int, default=700)
    parser.add_argument("--once", action="store_true")
    parser.add_argument("--show", action="store_true", help="show retained event summary")
    args = parser.parse_args()
    if args.interval <= 0 or args.slow_ms <= 0:
        parser.error("interval and slow-ms must be positive")
    connection = open_database(args.database)
    if args.show:
        for timestamp, kind, details in connection.execute(
            "SELECT time, kind, details FROM events ORDER BY time DESC LIMIT 40"
        ):
            print(time.strftime("%Y-%m-%d %H:%M:%S %z", time.localtime(timestamp)), kind, details)
    else:
        monitor(connection, args.interval, args.slow_ms, args.once)


if __name__ == "__main__":
    main()
