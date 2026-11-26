# Development 0.1.6

2026-10-15 (Asia/Shanghai). Development engine `0.1.6`, Flutter `0.1.6+7`,
MSIX `1.1.6.0`. Stable and Development use separate version series.

Development 0.1.6 includes the dev branch fix for SQLite metadata writers:
Database clones serialize complete metadata updates, including inherited pin
recalculation, so competing writers do not repeatedly overtake a waiting clone.
Reads remain concurrent; SQLite timeout, schema and recovery state are preserved.
The failure-release regression and repeated native Windows tests remain enabled.
Mechanism and limits: [TD-20261015-16](incidents.md#td-20261015-16同实例元数据写者竞争耗尽-sqlite-锁等待).

## Downloads and packaging

- Linux Flutter desktop: `twodrive-development-0.1.6-linux-x64.tar.xz`.
- Windows portable desktop: `twodrive-development-0.1.6-windows-x64.zip`.
- Windows developer test: `twodrive-development-0.1.6-windows-x64-test-unsigned.msix`.
  This maps to `1.1.6.0` and uses Microsoft's reserved unsigned test identity.
  Windows 11 administrators can explicitly install with `Add-AppxPackage
  -AllowUnsigned` and the Microsoft Desktop VCLibs dependency. No signing
  certificate or Store identity is supplied; the ZIP is available for ordinary use.
- GNOME OneDrive client: `twodrive_0.2.14+dev0.1.6-1_amd64.deb`, with its debug
  package, quilt source package, locked vendor component, build information and
  upload manifest. This package contains the GNOME client from dev, including the
  SQLite fix; Flutter Development remains in the portable bundles/MSIX.

Debian packages use `dpkg-buildpackage --build=full` and debhelper with generated
runtime dependencies, system SQLite, manuals, copyright/license material and
conffiles. Compilation/tests use a fresh Cargo home offline. Manifest checks cover
source, binary and debug outputs. Disposable upgrade/remove/reinstall/purge tests
check synthetic user state and opt-in service behavior. Ubuntu upstream CI uses
an explicitly documented rustup compiler dependency-check bypass; distribution
builds must satisfy Build-Depends without it. Debian archive acceptance and a full
Debian-packaged Rust dependency/license audit are not claimed.

## Verification

The release must remain a draft until final-source Linux/Windows desktop, native,
QUIC, Windows preview and Debian workflows succeed and every uploaded asset size
and SHA-256 matches the local payload. `verification.json` and `SHA256SUMS.txt`
accompany the release and identify the exact source commit and CI runs.
Runtime checks are recorded separately and only claimed after execution.
Mock/local tests do not establish live OneDrive/Google Drive, independent-network
NAT, Store certification or a Debian archive upload.

本版修复同实例 SQLite 元数据写者竞争，并沿用完整离线 Debian 源码/二进制
标准构建。Deb 为 GNOME 客户端，Flutter 桌面提供 Linux/Windows 便携包和
明确标注的未签名开发测试 MSIX。实际检查结果与边界以 Release 附件为准。

The application/package build input is
[`e64bd76485c225c460c8189f54477a9e22728a3f`](https://github.com/LumiaBlack51/twodrive/commit/e64bd76485c225c460c8189f54477a9e22728a3f).
Later evidence-only documentation does not alter those build inputs. The
[direct-source Debian workflow](https://github.com/LumiaBlack51/twodrive/actions/runs/38739280494)
passed full offline compilation/tests, archive/manifest checks, Lintian,
upgrade/remove/reinstall/purge and extracted-source clean/vendor validation.
The [native checks](https://github.com/LumiaBlack51/twodrive/actions/runs/38739010221)
and [WebDAV/QUIC tests](https://github.com/LumiaBlack51/twodrive/actions/runs/38739010210)
also passed. Final desktop/preview results and downloaded artifact checks are
recorded in the release verification report after execution.

Packaging references: [Debian Policy, source packages and rules](https://www.debian.org/doc/debian-policy/ch-source.html),
[Debian Policy, shared-library dependencies](https://www.debian.org/doc/debian-policy/ch-sharedlibs.html),
[Microsoft unsigned MSIX test packages](https://learn.microsoft.com/en-us/windows/msix/package/unsigned-package).
