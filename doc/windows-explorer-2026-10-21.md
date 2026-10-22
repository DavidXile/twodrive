# Windows Explorer integration — generated Provider only

This branch starts at dev `507fef1` and explicitly includes the unmerged
[PR #39](https://github.com/LumiaBlack51/twodrive/pull/39) dependency `7f98410`.
It extends that engine/journal/IPC implementation, without republishing alpha.3.
The production native mount, real-account native writes, rename/delete,
conflict resolution, persistent root restart and private-cache eviction remain
capability-disabled. This is not a completed Windows synchronization release.

## Explorer and engine ownership

The opt-in generated host registers its new guarded root with
`StorageProviderSyncRootManager`, alongside CFAPI. Each root has a unique
per-user registration ID. No existing root registration is enumerated or changed.
Shell registration is verified by both its ID and folder binding before removal.
There is no COM extension installation, global icon overlay or Explorer restart.

Windows derives availability and pin icons from the actual placeholders. A custom
Shell property separately reports acknowledged generations, pending recovery,
real read failures and active hydration. No successful download is relabelled as
an uploaded generation. Unknown state is explicit. The generated read-only
Provider cannot produce a cloud write conflict; no synthetic conflict is shown.
Root summary counts native resident files, private cache copies and pinned intent
separately; the worst observed child sync condition is preserved.
Pending/error messages are custom text properties. Native pending/error glyphs,
custom icon handlers and physical Explorer refresh remain unaccepted.

The owner observes only its bounded generated leaves every 500 ms, using a deadline
that is not reset by incoming status traffic. Failed intents remain pending with
classified failures and capped retry backoff; closing a vetoing handle or restoring
the generated source does not require toggling the intent again. Dirty recovery
remains blocked. The OS pin state is durable; the retry ledger itself is in-process,
and persistent root restart is still disabled. Native Shell
pin/free-space attribute changes request the same existing engine-controlled
read/dehydration operations. The OS cannot automatically dehydrate: the policy
does not enable AutoDehydrationAllowed, and external dehydration notifications
remain denied. A free-space intent still requires the exclusive CFAPI reference,
journal/file locks, acknowledged digest, generation, and clean/pin/handle checks.
Private cache eviction stays false.

Flutter negotiates the existing schema/scope and individual pin/offline-test/
callback-reconnect capabilities. It uses private IPC for all operations, and
opens only the engine-returned generated root through the existing file-manager
helper. Reconnect here means disconnect/connect of this live callback session;
it is not persistent process/OS restart recovery.

## Exit must preserve files

Microsoft documents that `CfUnregisterSyncRoot` **permanently deletes unhydrated
placeholders**. The earlier unconditional shutdown was unsafe for an untouched
fixture. Before explicit stop or normal owner destruction, this host retains its
deterministic generated data, without using a real account or network. The native
Session additionally refuses either Shell or CFAPI unregistration while a known
placeholder is incomplete or has changed logical length. It then holds exclusive
ordinary share-none read/WRITE_DAC leases to all known generated files, rechecks the metadata, and
reverts them to ordinary files before unregister. A concurrent later write is
therefore an ordinary-file write, not an unhydrated placeholder deletion. The
first real-CFAPI end-to-end run failed and retained its registration. An explicit
recovery using these leases preserved all bytes, recorded native dirty state,
converted the three complete files and unregistered successfully. The revised
normal-exit concurrency test subsequently passed once at source 9be73fe on a separately approved new root. A dirty incomplete
file is kept for recovery; shutdown reports incomplete cleanup instead of
claiming that unregistration succeeded. Generated files, journal and cache remain.
This fixture teardown policy is not a production sync-root uninstall policy.
A refused stop before conversion keeps the owner serving; partial teardown disables
native capabilities and reports cleanup and registration as unconfirmed. Drop
closes the owner channel before joining. See incidents TD-20261021-08 through 11.

## Acceptance matrix

| Scenario | Evidence / limit |
| --- | --- |
| Engine generation/digest/identity/open-handle/dirty guards | Existing actual-source regressions retained; final results recorded in PR |
| Failed read clears only after successful retry | Actual EngineSource regression |
| Handle-close and offline recovery retries; continuous status traffic; refused stop owner | Old-loop failures reproduced; corrected non-registration regressions pass locally |
| Error/pending/unknown folder aggregate vs separate residency/cache counts | Actual engine-source regression |
| Flutter pin/offline/reconnect capability gates and late-client isolation | Widget/client tests; final results recorded in PR |
| Shell registration/property metadata does not hydrate content | Source 9be73fe recheck passed zero initial fetches and property publication; native visual refresh remains unverified |
| OS Chinese/long-path reads, pin/unpin, release, offline retry and live reconnect | Source 9be73fe real generated-root recheck passed once; original 0cc1d4d failure remains recorded separately |
| Untouched placeholders survive normal exit | Real Windows build 26300, source 0cc1d4d: passed; all three 64 KiB files exact verified after unregister |
| Concurrent writer during all-file protection, conversion and unregister | Source 9be73fe real test passed: writer refused under all-file leases, resumed after ordinary-file conversion, exact complete bytes retained after unregister |
| Recovery of failed generated root | Existing root only: no registration/connect/download; three complete copies backed up, modified bytes durably vetoed, converted and unregistered; all six original test files exact verified |
| Explorer cloud/check/pin/pending/error visuals and context-menu interaction | Unverified: no supported isolated native screen/input under the user's foreground restriction |
| Actual GUI and engine IPC | Earlier 9be73fe ZIP digest/downloaded-engine IPC and empty-state GUI capture passed. Registered-root GUI interaction remains unverified |
| Explorer thumbnail/indexer/AV hydration behavior | Unverified; metadata-only API checks do not establish this |
| Persistent process/root reconnect, native write/rename/delete/conflict | Disabled/unimplemented |
| Existing OneDrive/Google Drive directories/cache | Read-only metadata preflight only; no adoption, client stop, credentials or private-cache access |

Follow-up verification is pinned to code **0c746ccfde425aee897a14da863307161b85f2bf**;
this final record changes documentation only.

| Follow-up check | Actual result and limit |
| --- | --- |
| All seven CI workflows | Passed at 0c746cc; desktop required one unchanged-source failed-job retry |
| Server adapter | Strict Clippy, 76 Rust executions, PNG byte decode, release build and probe packaging passed |
| Server outside-root metadata evidence | Six Python guards passed; one real metadata check explicitly skipped for confirmed no filter instances + INVALID_FUNCTION. Unknown roots remain refused; no fixture content read |
| Independent PNG bytes | Both CI platforms decoded generator stdout: exact64KiB, CRC/zlib/all pixels and SHA de207d01… passed; native_acceptance=false |
| Ordinary tree/layout | Linux CI passed; supported local Windows directory/full PNG decode passed. Unsupported Server directory validation was not relabelled as successful |
| Existing native tray smoke / unsigned MSIX | First hosted foreground transfer failed; same-source retry passed both. Neither result establishes local Explorer acceptance |
| Driver/root/foreground operations | Proposed CI filter attach was denied before commit/push and never executed/retried. Workflow is read-only for driver state; no new registration or local foreground action |

See incidents TD-20261021-15/16 for original failures, rerun evidence and limits.

No ignored registration test runs automatically in CI. All actual native tests
must use separately approved new generated roots, retain files and verify both
Shell and CFAPI cleanup. Previous task roots and approvals are never reused.
Screenshots, when available, must identify the exact source SHA and Windows build;
API or handle state alone is not visual acceptance.

The follow-up [visual acceptance preparation](windows-explorer-visual-acceptance-2026-10-21.md)
adds a real PNG, one directory, bounded host observations and exact-window evidence
capture. It has no new registration or visual pass. The user now requires an
isolated virtual screen while coding; no supported independent native display is
currently callable, so foreground interaction is paused. The new directory
retention path also still requires separately approved native acceptance.

Three separately approved roots have each been registered once and are now all
unregistered. All nine files are exact verified. They must not be registered
again or removed as build cache. The third root rechecks the corrected operation
and concurrency chain; it does not erase the first full-run failure.
The native failure and sanitized evidence are recorded in
[the native result record](windows-explorer-native-results-2026-10-21.json).

Primary references:
[Shell registration sample](https://github.com/microsoft/Windows-classic-samples/blob/main/Samples/CloudMirror/CloudMirror/CloudProviderRegistrar.cpp),
[custom item properties](https://learn.microsoft.com/en-us/uwp/api/windows.storage.provider.storageprovideritemproperties.setasync),
[hydration and dehydration policy](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfregistersyncroot),
[unregistration data semantics](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfunregistersyncroot).
