# Development 0.1.2 发布与验证

日期：2026-10-14。最终编译输入为已推送的 [863bf6d](https://github.com/LumiaBlack51/twodrive/commit/863bf6d6218853d79eecb57be6be8c8c2a4f6cef)。后续交付记录仅更新文档；源码标签和资产 SHA-256 见 Release 的 `verification.json` / `SHA256SUMS`。[Development 0.1.2 Release](https://github.com/LumiaBlack51/twodrive/releases/tag/dev-v0.1.2) 已于 2026-10-14T11:28:53Z 公开发布，预览版不取代稳定版 Latest。发布源码标签指向 [d39b32e](https://github.com/LumiaBlack51/twodrive/commit/d39b32ea6d3aaff1dfc096608defcfc593d84437)。6 个远端附件的 SHA-256 均与本地一致，Linux/Windows 原生程序、DLL、图标仍与 CI 原件逐项一致。

## 源码与分发

新增 Google Drive 桌面 OAuth 与普通文件读写，OneDrive 使用现有 Graph 实现和独立 Development 状态；WebDAV/NAS、配对设备及签名 UDP 局域网发现使用同一连接入口。在线添加/管理非活动配置不再要求断开当前账号；持久化失败保留原列表、选择和活动 revision。

GNOME/Zorin 用真实图标边界定位 X11 与原生 Wayland 状态面板。发布包包含 TwoDrive 专用 GNOME 组件、安装器、安装/升级说明，以及[第三方提供者、登录、上传和配套组件规范](provider-integration-spec.md)。首次安装 GNOME 组件可能须下次桌面登录加载。没有为本轮发布重启用户 GUI 或稳定服务、激活窗口或移动用户鼠标。

Linux/Windows 程序取自同一桌面 CI，原生程序/DLL/图标保持 CI 字节。组装时文档采用发布源码，Windows 文本行尾统一为 LF；安装脚本除行尾归一化外内容不变。源码归档由发布标签的 `git archive` 导出；不包括运行配置、令牌、用户 Google JSON、设备身份或工作区构建缓存。

## 实际验证

| 验证 | 结果与边界 |
| --- | --- |
| [Linux/Windows 桌面](https://github.com/LumiaBlack51/twodrive/actions/runs/38553734864) | fmt/严格 Clippy、Linux 64 项/Windows 58 项 Rust、IPC 文件/备份/在线配置管理、Flutter analyze 与两平台各 14 项、原生构建通过；Linux Python 7 项、C++/GJS 几何通过 |
| [Development DAV/QUIC](https://github.com/LumiaBlack51/twodrive/actions/runs/38553734826) | 两平台存储测试、release 与网络 smoke 通过 |
| [Peer/根工作区](https://github.com/LumiaBlack51/twodrive/actions/runs/38553734859) | Linux 整个 workspace、Windows core/backend/peer、原生更新进程测试与严格 Clippy/release 通过；默认忽略项不视作执行 |
| [Windows 双预览](https://github.com/LumiaBlack51/twodrive/actions/runs/38553734818) | 原生 Full/Lite 构建、engine IPC、包内 QUIC 和相同工具哈希通过；不是本 Development GUI 的云端 OAuth 验证 |
| Linux 最终 CI 包复验 | 包内引擎再次完成实际 loopback WebDAV 2 MiB 字节/哈希往返、条件上传/备份恢复/冲突保留、在线管理；实际本机签名 UDP 发现、可信邀请/QUIC 读写/重连通过 |
| 原生托盘 | GNOME 46 私有 Wayland 与 Xvfb/X11 会话各三轮稳定定位；Windows CI 原生合成 Shell 消息、外部焦点与 Escape 通过；没有向用户前台发输入 |
| 真实 Google | 本日较早本地开发构建的普通文件、8 MiB 分块/条件覆盖/旧版本拒绝、改名/回收站删除、真实 Linux FUSE 与备份通过 |
| 真实 OneDrive | 本日较早独立私有登录副本的 8 MiB 普通文件往返、条件覆盖/旧版本拒绝、改名/删除通过；没有更改稳定账号/服务 |

真实云端报告不是用最终 CI 程序再次执行的 OAuth/云端测试；网盘协议代码自初始特性提交后未改，最终改动是配置落盘失败保护和测试/分发。详细证据：[发布验证摘要](development-0.1.2-validation.json)、[Google](google-drive-dev.md)、[连接](storage-connections-dev.md)、[托盘](tray-positioning-2026-10-14.md)及[故障记录](incidents.md)。

## 尚未验证的范围

用户实体桌面的多屏/缩放与硬件点击、其他 Wayland compositor、Windows 实际 Google/OneDrive 浏览器登录与云端往返、组织/共享盘、实体 NAS 和任意跨公网 NAT 打洞未验收。Google 在线文档是网页入口，Windows 没有 FUSE 挂载驱动。

初期 Windows 一次配置替换 Access denied、另一个数据库 busy 失败在后续原生运行未复现；根因仍未确认，不把重跑通过描述成机制修复。已证明修复的是失败后不发布内存配置、跨平台路径断言/CI 依赖，以及上传竞态测试的明确同步。没有承诺所有同类系统失败永久消失。

English: Native Linux/Windows assets come from commit 863bf6d and the linked successful CI. Final assembly changes documentation and normalizes Windows text line endings only; native payloads remain unchanged. Live cloud reports are earlier local development checks, distinct from CI mocks and isolated compositor tests. Credentials are never shipped. See the JSON evidence and the stated unverified scope.
