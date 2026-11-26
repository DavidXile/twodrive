# Windows CFAPI isolated adapter, 2026-10-21

Baseline: dev `507fef1` (PR #35). This is an **opt-in experiment**, not a
production Windows sync release or an upgrade to the installed desktop.

The [published 0.1.7-alpha.1 CFAPI ZIP](https://github.com/LumiaBlack51/twodrive/releases/tag/dev-v0.1.7-alpha.1-cfapi)
is only the 122KiB native probe at `53991d5`, not a complete GUI feature. Its
exact-commit Windows/Linux adapter/native, Debian policy and Debian package CI
all passed. Subsequent work in PR #39 connects **metadata-only preflight** to
the real `apps/dav` engine and Settings UI. That connection does not enable a
native sync root, import, uploads, or Explorer status integration.

## Desktop IPC and UI connection

`desktop_local_files.rs` composes the same adapter classification through the
existing private JSON-lines IPC. `status.local_files` advertises schema version
1 and Windows `metadata_preflight`; `native_mount` and `import` remain false.
`local_preflight` accepts `schema_version`, one absolute `source_path` and an
explicit `source_kind`. Its response contains a decision and safety/result
flags, never the path or account identity. Unknown schemas are rejected; older
engines omit the capability and the UI hides the new card. Provider capabilities
and the existing Linux FUSE mount contract are unchanged.

Settings offers a single-file preflight form. Flutter only sends IPC and renders
the engine decision. It does not read file bytes, inspect Windows attributes,
register roots or implement a provider. A complete-file candidate is never
presented as uploaded/synchronized. Unknown provenance, private cache and virtual
mount classifications return without even opening the supplied path. No import
button or native Windows mount button is enabled.

Mutating CFAPI APIs are compile-time gated behind `fixture-probe`, with no default
features. The desktop dependency does not enable that feature, so it cannot call
fixture registration/dehydration. Its preflight does not substitute hard-coded
pending/open flags or caller-provided Cached labels for a real engine journal.
Production release stays disabled until an item-lock/generation/journal bridge
and queued-upload/open-handle/stale-version recovery tests exist.

Local checks for this connection: 12 Rust tests (including the actual desktop
boundary via a lightweight harness), strict Clippy, Flutter analyzer and 38
Flutter tests passed. Full desktop build/real-process IPC results are recorded
after CI execution; the prototype release must not be cited as proof of those
later changes. `scripts/test-local-file-preflight.py` exercises the actual engine
in generated isolated state without accounts or sync-root registration.

## Current implementation and next steps

The current `apps/dav` engine has Linux FUSE composition in
`experiments/twodrive-dev/src/mount.rs`; Windows exposes ordinary file transfers
and upload-only backups, with mounting disabled. The older `apps/full` engine is
another preview. Neither implementation contains CFAPI registration/callbacks.
The older preview statement was checked against the current dev source.

This slice adds `experiments/windows-cfapi`, a separate native adapter/probe. It
uses `twodrive-core::FileState`, rather than inventing a second synchronization
state machine. It imports no provider, protocol, OAuth, Flutter or peer transport
code. The source returns bounded, generated immutable content by opaque identity.
No real cloud service is contacted. It is deliberately not enabled in the GUI.

1. Validate CldApi registration, real placeholders, normal ReadFile hydration,
   pin intent, clean dehydration, dirty-data veto and failure/reconnect behavior
   in a newly created fixture root.
2. Add the durable engine item-lock/generation/journal bridge and cancellation;
   keep provider identity, conditional writes and recovery in the shared engine.
   Do not connect real writable roots before this work is reviewed.
3. Add capability-negotiated desktop IPC and native Explorer integration;
   validate real Explorer status columns/icons, thumbnails and restart recovery.
   Add delta/rename/delete support after durable mutation recovery is connected.

Linux lessons retained: a downloaded cache is not an uploaded generation; pending
parent moves/deletes and open writes veto release; conflicts preserve local bytes;
metadata/thumbnail probes should not hydrate content; account binding and local
identity must survive renames. See `twodrive-fs/src/{hydration,recovery,probes}.rs`,
the incident history and [provider boundaries](provider-boundaries-2026-10-13.md).
This first probe exercises only the documented subset, not Linux FUSE parity.

## Safety and usage

Build with one job on memory-constrained hosts:

```powershell
cargo test --manifest-path experiments/windows-cfapi/Cargo.toml --locked -j 1
cargo clippy --manifest-path experiments/windows-cfapi/Cargo.toml --all-targets --locked -- -D warnings
cargo build --manifest-path experiments/windows-cfapi/Cargo.toml --features fixture-probe --release --locked -j 1
```

Running the executable with no arguments only queries CFAPI platform information
and prints versioned JSON capabilities. No registration or account access occurs.
After explicit approval, `--allow-isolated-registration NEW_ABSOLUTE_FIXTURE_ROOT`
runs the native scenario. Its root leaf must start `twodrive-cfapi-fixture-` and
must not exist. It checks original ancestors under retained directory handles and rejects reparse points, managed sync roots and unknown query errors. A new leaf is created atomically relative to the locked parent. These hardened guards passed generated-directory tests; the revised full registration scenario subsequently received explicit approval and passed all eight native checks; see [guarded evidence](windows-cfapi-guarded-validation-2026-10-21.json).
Never supply a real cloud directory. The fixture contains three 64KiB generated
objects; one is deliberately edited without upload. Exit disconnects/unregisters
only its newly created root and leaves generated files available for inspection.
Cleanup failures are reported. The executable never recursively removes data.
If the process is forcibly terminated, registration can remain: inspect that
exact generated root and its provider identity before any manual recovery.

The library's fixture-only release guard requires `Cached`, no pin/pending work/open handles,
zero modified bytes and CFAPI in-sync confirmation, while holding an exclusive
CFAPI oplock. The prototype knows only its immutable fixture's engine state;
production release is disabled until the actual journal/generation lock is bound.
No API marks edited content as in-sync to make the test pass. Automatic OS
dehydration is not enabled.

`LocalStatus` separates `online_only`, `partially_available`, `locally_available`,
`pin_pending` and `pinned` residency from `pending_sync`, `conflict`, `error` and
`in_sync`. A complete download establishes local availability only. The probe
output is diagnostic JSON, not a new desktop IPC transport or a UI success icon.

## Acceptance matrix

Host: Windows 11 build 26300, x64; Rust 1.98.1. Read-only CldApi platform query
reports build 26100, integration 1576 (different from the OS build).

| Check | Result and evidence |
| --- | --- |
| Native compile, format, strict Clippy, release executable | Passed locally |
| Shared FileState release veto/range boundaries/status separation | 3 local unit tests passed |
| Migration planning incl. real generated ordinary file | 3 local Windows tests passed |
| CFAPI platform/capability query, no registration | Passed on this Windows host |
| Placeholder creation and actual ReadFile FETCH_DATA | Passed on this host in the approved generated root |
| Attribute-only metadata does not hydrate | Passed; callback count unchanged before real reads |
| Chinese and long (>260 total UTF-16 path) file reads | Passed; normal native reads matched all generated bytes |
| Pin/clean release/dirty release rejection | Passed natively, including stale Cached label vs OS dirty bytes |
| Offline failure, retry, disconnect/reconnect | Passed natively; cached bytes survived disconnection |
| Whole process/Windows restart recovery | Not implemented/verified; no host restart performed |
| Explorer icons/status columns/context menu | Not implemented/verified; no screenshot acceptance |
| Explorer thumbnails/indexer/AV unintended hydration | Not verified; attribute-only probe is insufficient |
| Persistent writable journal, rename/delete/delta/conflicts | Not connected to Windows adapter |
| Live OneDrive/Google Drive or original cache takeover | Not attempted |

CI compiles/tests on Windows and Linux. It never registers a runner's sync root
automatically; CI green does not establish native hydration or Explorer acceptance.
Release verification identifies the exact unmerged [PR #39](https://github.com/LumiaBlack51/twodrive/pull/39) source SHA separately.
The native scenario returned 8 passed checks, 6 FETCH_DATA callbacks and 2
expected offline-failure callbacks. Explicit cleanup succeeded; a subsequent
read-only sync-root query returned `0x80070186` (not a registered sync root).
All generated files were retained. See [sanitized evidence](windows-cfapi-validation-2026-10-21.json).

## OneDrive / Google Drive migration boundary

Only read-only client/process inventory was performed: OneDrive is running; no
GoogleDriveFS process was observed at that checkpoint. This is not a complete
inventory of accounts, roots or caches. No private file list or credentials were
read, no original root was registered/modified, and no client was stopped.

`migration::preflight` reads one explicitly supplied path's metadata without
following reparse points or opening content. It records no path in its result and
does not enumerate directories. Caller-provided source provenance is mandatory:

| Source observation | Planning result |
| --- | --- |
| Ordinary complete file or apparently full managed file | Needs an approved source/target/range/conflict/rollback/space plan; not automatic import |
| Reparse/offline/recall/sparse/encrypted managed item | Export/hydrate through original client only after approval; no direct adoption |
| Private cache, including apparently complete fragments | Unsupported; no assumptions about portability or upload completion |
| Google Drive virtual mount | Unsupported by file-copy preflight; requires an approved original-client export |
| Unknown provenance | Fail closed |

The classification is conservative: it does not prove cloud upload completion,
detect every virtual driver or parse another vendor's cache format. No import or
cache reader is implemented. A future ordinary-file import must preserve source,
use no-clobber destination writes and revalidate identity/size/mtime before commit,
with durable intent and rollback. Missing content, disk budget and unuploaded
changes must be resolved before any real migration. Credentials/tokens are never
an input to takeover. No additional provider is added in this slice.

API references: [registration](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfregistersyncroot),
[connection](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfconnectsyncroot),
[placeholder creation](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfcreateplaceholders),
[USN-aware in-sync state](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfsetinsyncstate).

## Subsequent durable cache bridge

The published alpha.2 remains the metadata-only GUI checkpoint at 7f0a77f.
Later PR commits add the [shared persistent journal and engine mock bridge](windows-local-journal-2026-10-21.md).
These commits do not enable production CFAPI/Explorer or writable GUI capabilities.
The guarded probe was separately approved and passed with full-byte dirty-file
verification, then unregistered; that does not prove the new journal is wired to
native callbacks or that Explorer visuals passed.
