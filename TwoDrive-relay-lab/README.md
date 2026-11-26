# Archived relay-lab reference

This directory preserves the original supplied `twodrive-relay-lab` 0.1.0
prototype for the [2026-09-16 peer audit](../doc/peer-implementation.md). Its
15 original files (98,438 bytes) are preserved unchanged. The nested README,
publishing scripts and CI template describe that historical experiment. The
CI template is not installed in `.github/workflows`, and this crate is not a
member of the TwoDrive workspace or a dependency of the desktop, daemon or peer.

The prototype has known cloud-manifest trust and control-replay gaps; see
[incident TD-20260916-02](../doc/incidents.md#td-20260916-02relay-lab-原型的云端信任与控制重放边界不足).
Its cloud discovery does not prove device ownership. Its quota check is not a
distributed reservation and does not guarantee a concurrent hard cap. The
original README's stronger claims and suggested deployment order are historical
statements, not current TwoDrive guarantees or deployment instructions.

The supported peer implementation resides in `crates/twodrive-peer` and reuses
provider services. Archiving this source does not import the prototype's OAuth,
Graph, VPS, file-transfer or trust logic into current services. No prototype
login, publication script, remote cleanup or cloud transfer was run during this
2026-10-13 archival task. Build caches, identities, tokens, private state and
relay messages do not belong in this archive.
