# Linux / Zorin adaptation for PR47

This branch starts at PR47 head `f4e07fe75bfb004e8d4591d1f9f6eff11391c4eb` (production code matches `0346913`). It incorporates the subsequent PR47 Baidu Content-MD5 fix through `960cf77e7511f70831aaded97be72c35e18f8d8e`. It adds Linux FUSE integration while retaining the shared UI, provider authentication, per-account write locks and storage identity checks. It does not replace the stable daemon.

## Behavior

- Retain simultaneous accounts and their mounts. Selecting an online account uses revision-bound `select`, preserves its last directory, and never implicitly authenticates.
- Keep native FUSE and its durable conditional-upload cache. The private Provider bridge uses Linux mount ownership/cleanup and platform-specific persistent caches for manual-edit adapters. Restart relocks writes; explicit local-password unlock remounts the affected account.
- Coalesce same-version file downloads without holding the shared cache lock over remote I/O. Reuse one live path lookup within each GET, and validate again on the next request. Complete contents remain verified before response headers, so cold reads can still show a preparation pause in the file manager.
- Keep active parent directories hot during child probes with a bounded 64-directory LRU (100,000 entries total). Linux read-only bridge navigation retains native directory listings for 30 seconds; editable and Windows mounts retain the 5-second policy. Display snapshot age remains capped at five minutes and real GET validation is unchanged.
- Answer filesystem capacity from a bounded display snapshot, with one separate background worker per bridge. Cold or expired quota never blocks Statfs/GIO classification; successful values refresh after five minutes and expire after thirty minutes, with a thirty-second failure backoff. Native capacity may remain unknown until its next query after the initial background result; account capacity cards are separate and unchanged.
- Bound read-only directory snapshots, coalesce refreshes, reject failed listings before successful DAV headers, and keep HEAD metadata-only. File downloads still validate live object/version/size. Editable views do not anchor writes to display snapshots.
- Validate Baidu content digests in both strict hexadecimal and standard Base64 formats; invalid or mismatched digests still fail.
- Transparent semantic cloud icons, bundled licensed Chinese/Latin fonts, responsive settings transitions, readable noncolliding mount names and Linux metadata-only preflight.

## Build and isolated checks

Use the prerequisites in [the desktop guide](linux-dav-desktop.md). Keep the full Linux bundle and its pinned Rclone component. No account state or external OAuth JSON is bundled.

```bash
cargo test --locked --manifest-path experiments/twodrive-dev/Cargo.toml --features mount --lib --bin twodrive-dav-engine --test writable_mount
cargo clippy --locked --manifest-path experiments/twodrive-dev/Cargo.toml --features mount --all-targets --release -- -D warnings
cargo build --locked --manifest-path experiments/twodrive-dev/Cargo.toml --features mount --release --bins
(cd apps/dav && flutter pub get --enforce-lockfile && flutter analyze && flutter test && flutter build linux --release)
python3 scripts/install-rclone-component.py --platform linux-amd64 --destination dist/rclone
TWODRIVE_RCLONE_BIN="$PWD/dist/rclone/rclone" python3 scripts/test-linux-readonly-mount.py --engine experiments/twodrive-dev/target/release/twodrive-dav-engine --peer experiments/twodrive-dev/target/release/twodrive-dev
TWODRIVE_TEST_RCLONE="$PWD/dist/rclone/rclone" cargo test --locked --manifest-path experiments/twodrive-dev/Cargo.toml --features mount --test bridge_fuse_linux -- --ignored
TWODRIVE_TEST_RCLONE="$PWD/dist/rclone/rclone" cargo test --locked --manifest-path experiments/twodrive-dev/Cargo.toml --features mount --lib native_statfs_and_gio_return_while_cloud_quota_is_stalled -- --ignored
python3 scripts/test-linux-locked-mount.py --engine experiments/twodrive-dev/target/release/twodrive-dav-engine --peer experiments/twodrive-dev/target/release/twodrive-dev
```

Tests use disposable synthetic accounts. Native mount tests require accessible `/dev/fuse` and `fusermount3`. Do not point test helpers at real account state.

| Locally executed validation | Result |
|---|---|
| Flutter model/widget tests and analyzer | 96 passed |
| Rust library / engine / editable bridge | 106 / 25 / 6 passed; 7 default ignored excluded |
| Strict release Clippy and Linux release binaries | Passed |
| Actual editable bridge FUSE | 1 explicit test passed |
| Actual read-only FUSE Statfs/GIO with held cloud quota, then capacity recovery | 1 explicit test passed |
| Dual read-only native mounts, copy to local and navigation with a paused upstream | 8 groups passed |
| Relock, pending bytes, explicit unlock recovery | 5 groups passed |
| Native saved-session restart | 6 groups passed |
| Concurrent online switching/control | 9 groups passed |
| Synthetic local QUIC sharing | 7 groups passed |
| Private GNOME/Xvfb window transitions | Passed; no exposed reconfiguration in fixed fixture |

These results do not establish hosted CI success, a Windows rebuild, real-cloud editing acceptance, or full Nautilus preview performance. iCloud/Baidu manual edits retain the documented check-to-publication race and do not gain atomic conditional-upload/backup capability. See incidents TD-20261029-33/34/35/36.

## Delivery

Published in the separate Linux branch via [PR #50](https://github.com/LumiaBlack51/twodrive/pull/50), ready for review. The local backend includes the latest Baidu digest fix and the read-concurrency/metadata optimization and bounded directory LRU/navigation retention; existing mounts were restored. The user confirmed successful client downloads and a slower successful mount copy before the read optimization, but reported that navigation still waited after the directory-cache change. The subsequent quota/Statfs fix has passed the isolated native boundary; deployment and real navigation acceptance are recorded in TD-20261029-36. The agent did not read private file contents for validation.

## 隐私范围

公开分支只包含程序源码、通用文档、合成测试和可分发字体及其许可证。真实账号、邮箱、授权凭据、OAuth客户端JSON、网盘列表/容量、缓存/数据库、主机路径、截图和本机部署报告均留在本地。默认启动只读，编辑密码由使用者在界面内自行设置。
