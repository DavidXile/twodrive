# Desktop provider boundaries (2026-10-13)

Development 0.1.1 separates the desktop client and shared file services from the
WebDAV implementation. The current registered adapters are HTTPS WebDAV and
paired TwoDrive devices. This change does not add OneDrive/Google Drive OAuth to
the Development desktop. The stable OneDrive daemon retains its existing backend.

| Owner | Responsibility |
| --- | --- |
| `provider.rs` | Provider capabilities, shallow browsing, bounded conditional upload, and path-to-opaque-ID file operations. A cloneable Backend delegates the existing CloudBackend contract to FUSE. |
| `profile.rs` | Provider-neutral account envelope; flattened opaque options preserve profiles.json compatibility. |
| `desktop_providers.rs`, `providers/` | Registration, public schema, identity/secrets, protected paths, routing probes, transport process, private worker recipe and cache binding. |
| `desktop.rs` | JSON IPC, account affinity/revision, subprocess ownership, cancellation, lifecycle and status. No DAV credentials or concrete backend type. |
| `desktop_backup.rs`, `mount.rs` | Provider-neutral backup recovery and generic FUSE composition. |
| Flutter | StorageClient IPC, schema-driven account form, provider labels and individual capabilities. |

To add a provider, implement Provider plus CloudBackend, register an adapter at
the composition boundary, and declare its fields, authentication identity,
secret fields and capabilities. Shared services and Flutter do not switch on the
provider kind. A provider must implement streaming cancellation and atomic
create-only/version-comparison upload before advertising conditional_upload.
Legacy CloudBackend defaults alone do not establish that guarantee.

Paths and remote IDs have distinct meanings. Metadata lookup selects a path;
download, rename and delete receive metadata.remote_id. Versions/cursors remain
opaque provider values. Backup reconciliation hashes the remotely committed
object before accepting an ambiguous intent; mismatching content stops progress.

Workers consume a private provider recipe bound to the account revision rather
than a protocol URL on the command line. Adapters restore connections and retain
existing DAV/account and peer/device binding strings. The stable state is not
migrated or adopted. Different account identities cannot inherit saved secrets;
profile-list IPC omits secret fields and unknown options.

A successful automatic physical-route fallback uses a yellow triangle warning
in Linux SVG and Windows ICO. Actual errors retain the error state and priority.
The client consumes structured route_fallback status; readable diagnostic lines
remain available separately.

## Validation and limits

The incident log contains the current results in TD-20261013-15/16/17/18. Tests use a
non-DAV provider with object IDs distinct from paths, exercise the actual IPC
handler and backup recovery, check atomic upload/pause/cancel protections, and
render a newly declared provider form without DAV/peer-specific fields. Two
isolated source mutations restore the original path-as-ID calls and must fail
behaviorally; they are not complete old-tree non-DAV implementations.

The original dev frontend rejects the new native-warning asset assertion;
MethodChannel/widget checks do not establish hardware interaction. Real
OneDrive/Google Drive end-to-end acceptance and additional provider adapters
remain outside this verification. No real account or installed client is changed.

Linux and Windows final CI passed on 4554021. The published Linux desktop
bundle was also exercised with an isolated real FUSE mount and a loopback DAV
server. Six normally ignored stable FUSE regressions passed using the packaged
CLI. These results do not establish live cloud behavior.

Delivery links and artifact verification are recorded in [incidents.md](incidents.md)
and the [sanitized verification report](provider-validation-2026-10-13.json).
