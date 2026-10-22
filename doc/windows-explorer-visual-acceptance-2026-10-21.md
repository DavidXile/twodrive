# Explorer visual acceptance preparation

This is preparation for one future interactive generated-root session. **No new
root was registered for this change.** The three earlier registrations are closed
and their approvals are exhausted. Their successful native API recheck at
`9be73fe060a5a8861761d0d8bdca3f6df2ab8532` does not validate this new directory
dataset, Explorer glyphs, menus, thumbnails or GUI actions against a live root.
See the [previous result matrix](windows-explorer-2026-10-21.md).

## Implemented preparation

The existing GUI option `--native-local-fixture-root` now starts `VisualV1` through
the same FixtureService, engine cache bridge and IPC. It creates exactly three
64 KiB files (196,608 bytes total) and one generated Chinese-named directory:

| Object | Placement | Content |
| --- | --- | --- |
| `visual-object-0` | Root, Chinese file name | Deterministic binary |
| `visual-object-1` | Generated child directory, short `.png` name | Valid 64×64 RGB PNG, exactly 64 KiB |
| `visual-object-2` | Same child directory, long binary name | Deterministic binary; independent long-path case |

The PNG has valid chunk CRCs, a zlib stream, legal text padding before IEND and
no trailing bytes. Its SHA-256 is
`de207d01b8cf2091ca00e4093aee8f4de45bd05e9974829593d41aba6385e7a6`.
Python independently decoded all pixels; Windows GDI+ decoded the ordinary file
and verified pixel (10,20) = RGB (40,80,120). A long-path PNG initially failed
GDI+ FromFile while the same bytes decoded through a stream. The PNG now has a
short path; long-path coverage remains a separate binary case.

`StandardV1` keeps its earlier file identities, content, root identity and cache
binding. VisualV1 has a distinct root and Provider/cache identity. Root-relative
WinRT lookup supports the one owned child directory without opening contents.
Its listing is marked initially complete only after all expected children exist.
Directory summaries remain distinct from file residency and private cache state.

The sibling state directory contains a versioned retention manifest binding the
volume, root file ID and every expected file/directory ID. Teardown leases all
complete files and then the directory, verifies the exact tree, and converts
children before their parent. All leases remain held through conversion. The
test-feature recovery command also supports a partial conversion: an ordinary
node is accepted only with the original persisted filesystem identity. It backs
up every actual complete file, verifies the copy, records dirty edits and then
converts remaining placeholders. It never registers, reconnects or downloads.
Unexpected entries, replacement IDs, incomplete data and unsafe handles refuse
cleanup. Files, journal, cache and backups remain available for investigation.

## Evidence sources

`native-phases.jsonl` is a bounded, fsynced, versioned observation log with ordered
sequence numbers, UTC timestamps, action results and periodic snapshots. It does
not store absolute root paths, credentials or caller command lines. It records:

- Provider bytes written, cache range bytes served, OS-requested bytes and bytes
  successfully transferred to CFAPI, as separate cumulative counters;
- per-file Provider/cache counts and a bounded ring of native requests with opaque
  object ID, caller PID, range size and result;
- native residency/pin flags, cache ownership, generations, pending intents,
  directory population and summary state.

Cache range counts exclude full-file integrity hashing IO. These counters measure
this generated host, not total filesystem IO. External thumbnail, indexer or
antivirus reads require time/PID attribution. A thumbnail handler may legitimately
request the complete small PNG; that alone is not a metadata-browsing failure.

The observer script `scripts/capture-native-fixture-evidence.ps1` verifies package
hashes, GUI PID/window, child engine PID/path/hash/root argument, current CFAPI
root binding and fresh phase data. Explorer capture additionally binds an exact
window handle, process and Shell folder/tab location. It uses DPI-aware target
PrintWindow and refuses minimized windows. It never restores, focuses, clicks,
starts an application, hydrates a file or claims that an API result proves a
glyph. Raw screenshots remain private until manually checked and redacted.

The observer's parser and GUI-only capture path were checked. An Explorer attempt
refused because the selected process main window was outside the fixture; an
explicit window handle is now supported. The full active-root observer is still
unexecuted. Earlier DPI-unaware capture was clipped at 150% scaling; DPI-aware
target capture produced the complete window. No Explorer or system restart was
used.

## Current interaction blocker

The user is coding on the foreground desktop and explicitly requires a virtual
screen. The current callable tools expose no supported independent native
screen/input session. UIA exposes Explorer items but no Flutter button tree;
earlier targeted Flutter child-window messages visibly changed Settings/Files,
but those probes predate the user's restriction and are not being repeated.
Ordinary Windows virtual desktops have not been established as isolated input
sessions and will not be used as a substitute. No driver or security setting is
changed. Background source work and tests continue without window interaction.

There are two remaining prerequisites: a supported isolated native session (or a
later user-authorized idle desktop window), and one fresh, specifically bounded
root approval. More permission alone does not supply an isolated display. Do not
consume another registration until the intended interaction path is available.

## One-session plan, not executed

Proposed new leaf: `explorer-visual-20261021` beneath the task directory; sibling
`explorer-visual-20261021.engine-state` and a separate backup/evidence directory.
Neither root nor state is created in preparation. Approval must cover exactly
the three generated files and one directory above, one CFAPI/Shell registration,
45 minutes of active testing, full data retention, conversion and unregister.

Before registration, verify no prior/uncertain attempt, sufficient memory, clean
source SHA, exact package hashes and isolated display. Do not reuse an old root.
Use the existing GUI launch arguments with a fresh GUI state directory. The
generated host uses a monotonic 45-minute deadline. At expiry it disables normal
actions and retries guarded retention/stop; a veto retains the owner and data
instead of forcing unregister. Retained-root recovery may outlive the active
test window and must be reported explicitly.

| Budget | Operation and required evidence |
| --- | --- |
| 0–5 min | Start once; record baseline before Explorer, source SHA, OS build, GUI/engine identity and zero/nonzero counters. Capture GUI and native cloud state in isolated session. |
| 5–12 min | Open root Details/Status column and file Properties separately. Compare byte counters and caller ring. Before reading the long binary, test offline uncached read and pin, restore source, and observe automatic retry. If an external reader already cached it, mark this subcase inconclusive. |
| 12–18 min | Open the child directory in thumbnail view, capture the actual PNG thumbnail, and attribute any content reads separately from prior metadata phases. Check directory summary against both children. |
| 18–28 min | Ordinary Chinese/long reads; native context-menu pin/unpin/free-space and matching GUI controls; observe actual cloud/check/pinned glyph transitions and refresh without restarting Explorer. Check native and private cache residency separately. |
| 28–35 min | Open-handle release veto then close/retry, cached offline read, callback reconnect and pending/error text versus actual native glyph. A complete generated edit must set durable dirty veto; no upload/conflict completion is fabricated. |
| 35–45 min | Close only test-owned readers/windows, retain/verify all actual file bytes, request normal stop, verify both Shell and CFAPI unregistration and ordinary file/PNG contents. Preserve raw errors, final errors, manifests, logs and backups. |

Stop new actions early for stale/missing observations, unexpected tree/identity,
session mismatch, resource pressure or interference with the user. Do not force
release/unregister or kill a retained owner. Recovery first requires complete
verified actual bytes under leases; unknown or incomplete content is preserved.
The earlier deterministic concurrent teardown regression remains recorded at
9be73fe. New folder teardown and its visual behavior need this fresh acceptance;
synthetic leases are not a substitute.

An ordinary-node recovery regression initially passed locally but failed on
Server 26100 because ordinary CfGetPlaceholderInfo errors differ. Recovery now
requires the bound filesystem identity and independently ordinary attributes/tag
state, rather than treating a particular failure HRESULT as proof. See
[TD-20261021-14](incidents.md#td-20261021-14-ordinary-retained-node-cfapi-errors-vary-by-windows-build)
for the failed CI job and corrected-source verification status.

Pending/error custom text is implemented. Native pending/error glyphs are still
unverified. Real cloud conflict UI, real Provider native writes/rename/delete,
persistent root restart and private-cache eviction remain disabled/unimplemented.
No real OneDrive/Google Drive directory, client, credential or cache participates.
PR #40's unrelated cache maintenance is not included or represented as released.

## Checks executed without registration

On Windows 11 build 26300, single-job builds: 76 test executions passed (14
adapter library, 24 desktop contract, 21 engine contract, 17 journal contract;
shared source tests occur in multiple harnesses). The two registration cases in
each relevant harness stayed ignored. Strict all-feature/all-target Clippy and
the non-test GUI feature compile passed. Ordinary-file regressions cover nested
Chinese/long WinRT lookup, exact-tree and filesystem-ID guards, directory/file
leases, durable phase sequences/budgets, owner expiry completion and separate
Provider/cache range counters with offline recovery. Independent Python and
Windows PNG decoding passed. CI verifies generated PNG bytes on both platforms;
Linux additionally validates the ordinary file tree. It does not register a root
or operate Explorer.

Server CI subsequently showed a Running CldFlt with no volume instances; every
root-info query returned INVALID_FUNCTION. The filesystem guard still refuses
this unknown state. CI checks generated PNG bytes directly from the internal
exporter's stdout; Linux additionally validates the complete ordinary tree.
The Windows metadata check is explicitly skipped only for the independently
observed no-instance condition, and is not a native acceptance pass. A proposed
CI filter attachment was denied before commit/push and was not executed; the
workflow performs read-only filter diagnostics. See incident TD-20261021-15.

All seven workflows at 0c746cc succeeded; the hosted tray focus failure required
one unchanged-source retry (incident TD-20261021-16). Server adapter release and
probe packaging passed with its native metadata evidence explicitly skipped.
The previously downloaded 9be73fe ZIP is an older generated-only preview;
0c746cc CI artifacts contain the preparation. No new GitHub Release was published;
alpha.3 remains the earlier PR #39 native-journal preview. These are experimental
generated-only artifacts, not stable/full Windows Explorer support.
