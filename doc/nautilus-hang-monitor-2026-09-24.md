# Nautilus responsiveness monitor (2026-09-24)

The local user service `twodrive-nautilus-watch.service` runs
`scripts/monitor-nautilus-hang.py` every five seconds. It checks the
Nautilus D-Bus Ping only while a Nautilus process exists. A Ping taking
at least 700 ms, failing, or timing out at two seconds causes a snapshot of
Nautilus and TwoDrive daemon thread states and kernel wait-channel names.
Repeated bad states are sampled at most every 30 seconds. Recovery,
process appearance/disappearance, and hourly healthy heartbeats are also
recorded.

The SQLite database is
`~/.local/state/twodrive/nautilus-watch/events.sqlite3`. The monitor
deletes rows older than 48 hours on each probe. The directory is mode 0700
and the database is mode 0600. It contains timestamps, PIDs, thread state,
wait-channel names, counts, and Ping times. It does not contain filenames,
paths, file contents, command lines, URLs, tokens, or raw journal messages.

Inspect recent events:

```sh
python3 scripts/monitor-nautilus-hang.py --show
systemctl --user status twodrive-nautilus-watch.service
```

When the desktop offers **Wait or Force Quit**, choose **Wait** if possible
and note the time, the directory type (ordinary local or TwoDrive mount),
and whether other file manager windows still respond. The monitor may show
whether Nautilus's main thread was blocked and whether the daemon was also
waiting. Kernel wait channels alone do not prove the application-level cause.
The saved timestamp can then be correlated with the existing Nautilus and
TwoDrive journals while redacting private data. If no Nautilus process
remains, a later probe cannot recover its stack.

To stop future monitoring without changing the TwoDrive daemon:

```sh
systemctl --user disable --now twodrive-nautilus-watch.service
```

The service is a local diagnostic installation; it is not part of the TwoDrive
deb package. This monitor does not restart Nautilus or the mount.

## Source archival check (2026-10-13)

The previously local-only script and this guide were committed and merged into
TwoDrive dev through [PR #12](https://github.com/LumiaBlack51/twodrive/pull/12). Eight isolated checks passed: private SQLite modes, public-directory
and public-file rejection, 48-hour retention, absent process reporting,
injected slow/recovered events, 30-second repeat throttling, input validation
before state creation, and a real read-only local probe into a temporary DB.
The slow/error cases use injected probe/process results and do not reproduce a
hung Nautilus. This task did not modify or restart the installed service, inspect
its private database, or run a live cloud operation. The script remains a
diagnostic aid; the original hang root cause is still unconfirmed.
