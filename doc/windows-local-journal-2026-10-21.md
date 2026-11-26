# Windows private-cache journal bridge

This extends the unmerged PR #39 after the published alpha.2 source checkpoint
`7f0a77f8b969f0c90aaf3b65a3072b783ee705b3`. That tag and its assets stay unchanged.
It does **not** enable a production Windows sync root or writable GUI capability.

The smallest end-to-end contract uses a generated, versioned mock object and a
private cache: download by opaque ID, recheck the provider version after EOF,
fsync the complete cache, atomically publish its metadata and journal, persist a
new generation before returning a writable lease, fsync on close, claim a
conditional upload, and acknowledge only the exact claimed generation/version
and content. No protocol, database or CFAPI logic lives in Flutter.

## Ownership and persistence

`twodrive-core::CacheJournal` extends the existing `Database`, stable local IDs
and `FileState`; it does not introduce another synchronization state enum. Its
table is created only when this opt-in owner is opened. Existing FUSE databases
and metadata-preflight IPC do not open the owner or adopt foreign caches.

The engine owns one fixed Backend for the session. Arbitrary-string construction
is test-only; no real-provider production constructor is exposed. A domain-separated digest of
the existing provider/account binding and the canonical private cache root are
persisted before ownership recovery. Another identity/root is rejected before
advancing the epoch or resetting leases. The composition boundary must use the
existing profile-revision/provider binding, never a UI label or fresh arbitrary
namespace; changing accounts requires a different private state. Binding values
and paths are not exposed by status. No alternate Backend can be supplied to a
hydrate/upload/reconcile call after opening the bridge.

An OS-exclusive lifetime lock admits one private-cache owner. All read/write/
upload leases retain that owner. Directory handles on Windows deny ancestor
replacement; original ancestors and opened cache handles reject reparse/offline/
recall items. Only direct children of the private cache can be accessed. This is
not an importer or support for another client's placeholders.

The owner gate, SQLite IMMEDIATE transactions and real cache file locks serialize
intent and mutation. Durable readers/writers and generation counters are queried,
not caller-provided flags. A write intent is committed before a writable handle
is returned. Closing fsyncs before making it Dirty; it never implies upload.
Base identity, path, cache binding and opaque version must still match. Pending
deletes or parent metadata operations veto upload/release. Legacy cache-prune/
release APIs refuse journal-owned records, even with a stale Cached label.

The initial bridge serializes these checks per cache owner, including bounded-memory digest verification;
it does not hold that gate during network transfer. File locks and the persisted
upload claim remain live throughout transfer. More granular item scheduling is
a future performance change, not permission to weaken the checks.

## Acknowledgement and recovery

`experiments/twodrive-dev/src/local_files.rs` is the actual shared-engine bridge.
It uses the existing portable model and Provider contract on Windows and the
existing shared contract on Linux. Providers must explicitly advertise atomic
conditional upload. Metadata and object IDs remain distinct from paths.

An error other than a definitive version conflict can be a remotely accepted
write with a lost reply. It leaves the upload intent uncertain and blocks blind
retry, edits and release. Restart does not clear that intent. Reconciliation
downloads the provider object and rechecks identity/version/size after EOF;
only full matching content can acknowledge the upload. An unchanged remote
version permits a conditional retry; a changed, mismatching version becomes a
conflict with local bytes preserved. No automatic conflict overwrite is offered.

After the owner process exits, startup can reset old handle counts only after
acquiring the exclusive owner lock. A vanished writer becomes Dirty with
recovery_required; the cache must be reopened, locked and fsynced before upload.
An interrupted upload remains uncertain. Missing cache data or inconsistent
bindings/counters fail closed. Tests use subprocess exit without destructors;
they do not establish power-loss, OS reboot, physical Explorer or live-cloud
acceptance.

`engine_release_ready` is a diagnostic, never a detached authorization. The
actual clean-cache callback holds the owner gate, SQLite write lock and exclusive
file lock. The engine compares the complete file digest under that lock; an
untracked edit invalidates the generation and becomes Dirty. A future native
adapter must additionally own/recheck a CFAPI oplock, modified ranges, residency,
pin state and in-sync evidence at the action. No native dehydration callback is
connected by this slice. Resident/downloaded, pinned, pending, conflict/error and
acknowledged generations remain separate concepts.

## Validation

Windows lightweight harnesses compile the actual model/provider/bridge source;
the full desktop CI also runs its engine tests. Generated files exercise:

- Download → real edit/fsync → conditional upload → same-generation acknowledgement.
- Real readers and the owner lock block competing writers and clean-cache actions.
- Dirty, queued upload, stale Cached/generation/version/cache binding and pending
  delete/parent move remain release vetoes.
- Accepted upload with lost reply, offline before commit, full-byte reconciliation,
  remote conflict and provider-version changes during download.
- Actual subprocess exit during a write/upload, reopen, preserved complete bytes,
  and no blind recovery upload.

Local Windows results: 37 adapter/IPC/journal/actual-engine harness tests, the
existing 40 core regressions, format and strict Clippy passed. The journal
includes two real subprocess exit/reopen tests. Removing only the ownership
veto from the actual legacy release function makes the intended regression fail;
restoring byte-identical source makes it pass. This is a controlled source
mutation with generated data, not a test of a released writable client.
Exact-head full desktop/Linux/Windows CI is recorded in the PR after execution. Native
fixture results are separate: the explicitly approved hardened probe passed all
eight checks; its generated root was unregistered and all three files retained.
See [guarded evidence](windows-cfapi-guarded-validation-2026-10-21.json).

Two independent-review regressions failed against the previous implementation:
same-length editing between an earlier hash and final reconciliation could
acknowledge stale content, and a rejected second hydration could replace owned
cache metadata. Reconciliation now hashes under its final file lease through
commit; hydration checks ownership before metadata mutation and serializes its
claim with publication. Both deterministic regressions pass after these fixes.

Remaining integration: real native callback cancellation/hydration and journal
ownership, stable item-status IPC/UI, Explorer state/icon/thumbnail behavior,
native rename/delete and conflict-resolution/restart scenarios. These remain
capability-disabled. Real accounts, installed clients, existing sync roots,
vendor caches and peer UDP/QUIC code are untouched.

## Opt-in native engine integration (after the consistency-fix checkpoint)

A separate `native-local-fixture` build feature now connects the existing CFAPI
adapter to this actual engine bridge. It requires an explicit
`--native-local-fixture-root` and a NEW dedicated desktop `--state`; normal startup
registers nothing. Only the internal generated read-only Provider can construct
this host. Profile/account and ordinary cloud operations are refused in the
fixture session. No arbitrary account-binding string becomes real identity.

The OS owns placeholder reads. FETCH_DATA passes an opaque per-file identity to
the engine, which downloads/version-checks/fsyncs/publishes into the journal,
opens a real shared cache lease, verifies its acknowledged digest under that
lease, and returns bounded 64 KiB chunks to CfExecute. CANCEL_FETCH_DATA and
shutdown set cancellation flags. Foreign identities and unknown states fail
closed. Rename/delete and external dehydration notifications are denied; this
is not a writable native mount. Each visible generated file is 64 KiB.

The explicit native-copy release holds a referenced exclusive CFAPI oplock,
then the journal gate, IMMEDIATE transaction and exclusive private-cache file
lock. It rechecks OS identity/modified ranges/pin/in-sync/logical length and the
complete acknowledged cache digest before calling CfDehydratePlaceholder.
Private cache bytes remain retained: `native_copy_dehydrate` is distinct from
`cache_evict`, which stays false. Eligibility booleans are not detached permits.
The protected reference is bounded to these small generated-file checks; there
is no network transfer inside it. See Microsoft's [oplock contract](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfopenfilewithoplock)
and [protected-handle lifetime](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfreferenceprotectedhandle).

OS-observed edits, length changes or an unreadable/replaced placeholder install
a durable native-dirty veto. It also covers an edited placeholder that was never
hydrated. Private-cache read/upload/generic recovery and dehydration cannot
clear it or substitute older bytes. Private journal schema 2 rejects the
unreleased schema 1 rather than letting an old binary ignore this veto. No real
data is migrated. Recovery of externally edited visible bytes is deliberately
not enabled by this slice.

`local_files.item_status` negotiates schema 1 with scope
`generated_fixture_only`; `local_items`, `local_read`, `local_dehydrate` and
`local_stop` use stable private IPC. The GUI presents residency, pin intent,
OS downloaded state, retained cache, sync status, generation and acknowledged
generation separately. Unsupported write/rename/delete/cache eviction remain
false. Unknown schema/scope and late results from a replaced client are refused.
Application controls are not evidence of Explorer shell icons.

| Acceptance | Current evidence |
| --- | --- |
| Engine download/version/lease/digest, dirty and restart veto | Local generated Rust regressions passed |
| GUI schema, separate residency/sync/cache and stale replies | Flutter analyze and 41 tests passed |
| New real OS-to-engine read and native release | Passed once on Windows build 26300, source aff4faa; root unregistered, files retained |
| Earlier hardened standalone adapter | 8 native checks passed; both old roots unregistered; does not validate this new bridge |
| Explorer shell icons and thumbnails | Unverified |
| Live Provider binding, writable native rename/delete/upload and external-edit recovery | Disabled/unimplemented |
| Existing OneDrive/Google data/cache migration | Preflight only; not performed |

The ignored end-to-end regression accepts only a caller-supplied explicitly
approved NEW root, creates three generated placeholders and new sibling private
state, exercises OS reads/offline retry/pin/open-handle/dirty veto, then checks
unregistration and retains all files. It is never run by CI automatically.
Existing two fixture approvals were consumed and are not reused.
The newly approved engine fixture passed the ignored native regression once.
It produced six real FETCH_DATA callbacks (two injected offline failures),
passed normal ReadFile through the bridge, native-copy dehydration, cached
offline reads, offline-miss retry, pin/open-handle veto and complete dirty-byte
preservation. After shutdown an independent read-only CFAPI query returned
0x80070186, all three complete files matched expected bytes and SQLite reopened
read-only with the persisted dirty generation 1 / acknowledged generation 0.
The registration approval is consumed. See [sanitized evidence](windows-native-engine-validation-2026-10-21.json).
This does not prove Explorer thumbnails/icons or production native root recovery.

The first integration CI also caught the newly enabled optional dependency
missing from the engine lockfile. Only the existing windows-sys 0.61.2 edge was
added; no package versions changed. Full exact-head CI remains required.
