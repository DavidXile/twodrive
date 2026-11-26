# Development 0.1.3 发布与验证

日期：2026-10-15。Linux/Windows 原生程序来自同一次[桌面 CI 38668500026](https://github.com/LumiaBlack51/twodrive/actions/runs/38668500026)，代码头 `8f4aa72e1e1b4d83c223fc8839a2c2dae40c8701`；源标签 `dev-v0.1.3` 指向已合入 dev 的 `bffbe0e7866df8b29b32050d2b7035e135ffb96f`，两树相同。[Development 0.1.3 Release](https://github.com/LumiaBlack51/twodrive/releases/tag/dev-v0.1.3) 已于 2026-10-15 10:19:03 +08:00 公开为预览版，6 项附件全部通过远端 digest/大小/状态和源标签校验；稳定 Latest 仍为 0.2.14-1。Linux tar SHA-256：`1e2c62620b9f82bd94d87449919fd35a98a3af3e0c4769a9460ac7496296ed50`；Windows ZIP：`b46e31a63476f19db300e2e027ff3669346e4f2dc393fa995617e9ac9d0bba61`。

## 用户可见变化

同步稳定 0.2.14 的安全 Graph/OAuth 诊断、删除待办持久退避，以及此前的本地 Dirty/Uploading unlink 修复。Linux 挂载状态显示本地已删除但云端待确认/被拒绝，提供重新尝试，并绑定当前显示的活动挂载 profile。待办、必要缓存和同路径屏障保留；没有扩权或改变删除目标。直接文件页删除仍同步返回错误，Windows 没有 FUSE。

## 实际验证

- 原生 Linux/Windows 独立 Rust：65/59 项；两平台 Flutter 各 16 项；严格 Clippy、格式和 Flutter analyze 通过。
- 两平台原生 IPC、文件/备份隔离、配置管理、原生 GUI 构建及 Windows 托盘烟测通过。
- [root CI](https://github.com/LumiaBlack51/twodrive/actions/runs/38668500005) 与[独立 DAV/QUIC CI](https://github.com/LumiaBlack51/twodrive/actions/runs/38668499972) 通过；本机 root 默认 173 项通过。
- Linux 最终 CI 包再次通过 loopback WebDAV/IPC 的 2 MiB 字节往返、条件覆盖/备份/冲突保留，以及本机签名 UDP、可信邀请、QUIC 读写和重连。
- Linux tar 保持 CI 原文件；Windows ZIP 所有 35 个文件与 CI 原件逐字节相同，三个 EXE 均为 PE x64，ZIP CRC 通过。Windows 文档也保留 CI 行尾；独立提供者规范附件采用源仓库字节。

## 边界与升级

没有用本版本进行真实 OneDrive/Google 云端验收；原旧对象 403 的原因和当前生产待办状态仍未检查。没有验证 Windows 用户实际云端 OAuth、组织/共享盘、实体 NAS、任意跨公网 NAT 或用户实体桌面的硬件交互。没有安装、启动用户 GUI、重启服务或读取私人凭据。

升级前退出 Development 并备份独立状态、未同步缓存和旧程序，核对 SHA256SUMS，按包内 INSTALL.md 升级。状态中的“重新尝试”只让保留任务到期，不代表云端完成。此预览版不替换稳定 Latest。

English: Native Linux/Windows assets share the same successful CI source. This preview adds safe diagnostics, durable deletion retry and explicit retained-deletion status for Linux mounts. No live cloud acceptance or production installation was performed.

发布采用 [校验保护脚本](../scripts/verify-release-assets.py)，默认只读，缺失/额外/上传未完成、大小/摘要/实际 tag 目标或通道错配都不会执行公开；6 项保护回归通过。先前稳定版临时发布顺序错误和恢复单独记于 [TD-20261015-02](incidents.md#td-20261015-02release-校验失败后仍执行公开操作)。本记录及发布保护脚本属于交付后的仓库更新，不改动原生 CI 客户端源码和已发布资产。
