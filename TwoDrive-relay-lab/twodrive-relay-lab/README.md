# TwoDrive Relay Lab

Experimental, standalone implementation of the **encrypted OneDrive relay** and **user-owned VPS relay** discussed for TwoDrive. It deliberately does **not** alter the stable TwoDrive filesystem, daemon, database, or current local installation.

## Security model

- File chunks are encrypted **before** they leave the sending device.
- Each device has a persistent Ed25519 signing identity and X25519 receive key.
- Each relay envelope uses an ephemeral X25519 key, HKDF-SHA256 and XChaCha20-Poly1305.
- Payloads are signed; received chunks are accepted only from valid same-OneDrive-account device manifests.
- OneDrive and the VPS see ciphertext plus unavoidable routing metadata (recipient device id, message id, expiry and byte length). They do not receive decryption keys or plaintext filenames.
- VPS access uses challenge-response HMAC with a user-provided relay token, so the token itself is not sent over the raw TCP connection.
- This is an experimental protocol. Production integration should receive a dedicated security review and protocol version freeze before being trusted as the only copy of important data.

## Relay-cache policy

### OneDrive

Default hard cap: **1 GiB**, plus a default **512 MiB whole-OneDrive free-space reserve**.

- Before every upload, expired relay objects are deleted.
- The sender also checks Microsoft Graph's drive quota. With the default `--reserve-free 512MiB`, it will wait rather than consume the user's last 512 MiB of OneDrive capacity. Set `--reserve-free 0` only if you intentionally want to disable this second guard.
- If `current encrypted cache + new encrypted chunk > hard cap`, the sender applies **backpressure** and waits for the receiver to drain verified chunks (default wait budget 30 minutes). Active, unexpired data is **not silently evicted**.
- After a receiver verifies, decrypts and durably saves a chunk locally, that OneDrive relay object is immediately **permanently deleted**. The lab does not fall back to ordinary recycle-bin deletion, because recycle-bin items still count against OneDrive storage quota.
- Explicit cleanup: `onedrive-clean`.
- Default message TTL: **24 hours**, allowed range 5 minutes to 7 days.
- Chunks are 4 MiB and envelopes are binary CBOR, so the relay cache stays close to ciphertext/raw-chunk size instead of paying Base64 expansion. Sending is streaming: only the current chunk/envelope is held for transport, so a large backup file is not materialized in memory.

This makes the limit a bounded **working buffer**, not a total file-size limit: a 50 GiB backup can pass through a 1 GiB relay cache while the receiver is running. You can choose another explicit cap with `--quota`, for example `512MiB`. The separate `--reserve-free` guard protects space used by the user's other OneDrive content.

### VPS

Default hard cap in the examples: **4 GiB**, configurable.

- Server stores only opaque encrypted envelope bytes.
- Expired messages are removed on normal relay activity and startup.
- Receiver ACK deletes a successfully verified message.
- Server rejects writes that would exceed its hard quota.
- Maximum individual relay message is configurable; default example is 8 MiB.

## Build

Requires a current Rust toolchain supporting edition 2024.

```bash
cargo build --release
```

The binary is:

```text
target/release/twodrive-relay-lab        # Linux
target/release/twodrive-relay-lab.exe    # Windows
```

The crate has its own `[workspace]`, so it can be copied to `twodrive/experiments/twodrive-relay-lab/` without modifying the main TwoDrive workspace or `main` branch.

The included GitHub Actions workflow builds/tests on both Ubuntu and Windows and uploads the native binaries as workflow artifacts. This is the easiest way to get a Windows `.exe` for the experiment without installing Rust on the Windows backup machine.

## First test: two devices on the same OneDrive account

Do this once on **both** Windows and Linux:

```bash
twodrive-relay-lab login
```

The command uses the existing public TwoDrive Microsoft application id and PKCE localhost callback. It requests the same `Files.ReadWrite`-class access already used by TwoDrive; tokens are stored only in this lab's per-user config directory.

Initialize each machine with a different name:

Windows PowerShell:

```powershell
.\twodrive-relay-lab.exe init --name "Windows Desktop"
```

Linux:

```bash
./twodrive-relay-lab init --name "Linux Laptop"
```

List peers:

```bash
twodrive-relay-lab peers
```

The public manifests live under the application's OneDrive App Folder (`control/v1/devices`). Secret keys and OAuth tokens are never uploaded.

## Test the OneDrive relay on MUST-WIFI

MUST-WIFI client isolation is useful for this test because the machines do not need LAN connectivity.

On Linux, send a disposable test file to the only other published device:

```bash
./twodrive-relay-lab onedrive-send \
  --to auto \
  --file ./relay-test.bin \
  --quota 1GiB \
  --reserve-free 512MiB \
  --ttl 24h
```

On Windows, start the receiver first and leave it watching:

```powershell
mkdir D:\TwoDriveRelayTest -ErrorAction SilentlyContinue
.\twodrive-relay-lab.exe onedrive-recv `
  --out D:\TwoDriveRelayTest `
  --quota 1GiB `
  --watch `
  --poll 5s
```

Then start the sender. If the bounded cache fills, the sender waits instead of exceeding the cap. The receiver verifies/decrypts each chunk before deleting the corresponding relay object from OneDrive. If verification or decryption fails, that object is **not** ACKed/deleted automatically.

Check/clean expired relay cache:

```bash
twodrive-relay-lab onedrive-clean --quota 1GiB
```

Emergency/manual purge of **all** relay ciphertext (including unreceived data):

```bash
twodrive-relay-lab onedrive-clean --quota 1GiB --purge-all
```

## Test a user-owned VPS relay

Build/copy the same binary to the VPS. Generate a long random token, for example:

```bash
openssl rand -hex 32
```

On the VPS:

```bash
export TWODRIVE_RELAY_TOKEN='PASTE_A_LONG_RANDOM_TOKEN'
./twodrive-relay-lab vps-serve \
  --listen 0.0.0.0:4433 \
  --data-dir ./relay-cache \
  --quota 4GiB \
  --max-message 8MiB
```

Open TCP port 4433 in the VPS firewall/security group.

On Linux sender:

```bash
export TWODRIVE_RELAY_TOKEN='THE_SAME_TOKEN'
./twodrive-relay-lab vps-send \
  --relay YOUR_VPS_IP:4433 \
  --to auto \
  --file ./relay-test.bin \
  --ttl 24h
```

On Windows receiver:

```powershell
$env:TWODRIVE_RELAY_TOKEN='THE_SAME_TOKEN'
.\twodrive-relay-lab.exe vps-recv `
  --relay YOUR_VPS_IP`:4433 `
  --out D:\TwoDriveRelayTest `
  --watch
```

In this first experiment, VPS message transport is independent from OneDrive, but **peer discovery/trust still uses the same-account OneDrive manifests**. The later no-login pairing flow can replace that trust bootstrap without changing the encrypted envelope format.

## What the VPS can see

It can see:

- recipient opaque device id;
- opaque message id;
- expiry time;
- encrypted message length;
- timing/IP metadata inherent to the network connection.

It cannot see:

- plaintext file content;
- plaintext filename (inside the encrypted payload);
- sender's private keys;
- receiver's private keys;
- OneDrive OAuth tokens.

## Current experiment boundary

Implemented here:

- same-OneDrive-account public device discovery;
- signed device manifests;
- compact binary CBOR envelopes (no bulk Base64 storage inflation);
- per-message authenticated encryption;
- signed payloads;
- chunked file transport;
- OneDrive App Folder mailbox;
- OneDrive hard cache quota and expiry cleanup;
- delete-after-verified-receive;
- user-owned VPS opaque mailbox;
- VPS hard cache quota, TTL cleanup and ACK deletion;
- challenge-HMAC relay authentication;
- Windows/Linux-compatible standalone layout.

Not implemented yet:

- NAT traversal / QUIC direct path;
- no-login invite pairing;
- production Windows Explorer/Cloud Files integration;
- selected backup-directory service mode;
- background service/tray UI;
- automatic route racing/migration (direct -> VPS -> OneDrive);
- TLS for VPS metadata privacy (file data is already end-to-end encrypted; production should still put the relay behind TLS or add native TLS).

## Recommended integration order

1. Run this lab on Windows + Linux using the same OneDrive account.
2. Verify encrypted OneDrive relay under MUST-WIFI client isolation.
3. Verify the VPS path.
4. Add a selected-directory backup service on Windows.
5. Only then wire this transport underneath TwoDrive's provider/service abstraction.
6. Add QUIC/NAT traversal later; the relay paths remain the fallback.

## Publish without touching the current TwoDrive working tree

The package includes `PUBLISH_EXPERIMENT.sh`. It creates a **temporary git worktree** from `origin/main`, adds only this experiment, installs a Windows+Ubuntu GitHub Actions build/test workflow, commits to `exp/encrypted-relays`, pushes that branch, then removes the temporary worktree.

Your existing `~/software/twodrive` checkout is not reset, overwritten, or switched to the experimental branch.

```bash
./PUBLISH_EXPERIMENT.sh "$HOME/software/twodrive"
```

The script intentionally refuses to overwrite an existing local or remote branch of the same name.
