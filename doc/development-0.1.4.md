# Development 0.1.4 发布与验证

日期：2026-10-15。[Development 0.1.4 Release](https://github.com/LumiaBlack51/twodrive/releases/tag/dev-v0.1.4) 已于 11:38:42 +08:00 公开为预览版，6 项附件校验通过，稳定 Latest 核实仍为 0.2.14-1。源码标签 `dev-v0.1.4` 指向 `791a006187856bb9d5eb81f3eca3990382f855a7`，与本次[原生桌面 CI](https://github.com/LumiaBlack51/twodrive/actions/runs/38675643955) 的构建提交一致。[PR #19](https://github.com/LumiaBlack51/twodrive/pull/19) 合入 dev 的提交为 `41929a9e36aaaa7b973df4ab8dcaaf59f2c057a8`，合并时两树相同。

## 用户可见变化

- 添加连接后直接提供“登录并使用”，其他连接提供“切换使用”，活动项标为“当前使用”。切换仍保留未上传缓存/账号绑定保护。
- Google/OneDrive 显示服务返回的账号名称，WebDAV 显示已连接用户名，设备共享显示连接名称；配置名单独保留。连接页、管理标题、托盘面板和原生托盘提示同步更新；离线配置注明上次登录。
- 合并状态与目录轮询，长操作完成主动更新状态，目录每 15 秒及窗口聚焦时刷新。状态请求错误自动恢复，失败文件操作仍保留提示。
- desktop IPC 队列保留上限与独立取消通道，取消后尚未执行的操作不再执行；持久同步待办继续由既有引擎处理。

## 已完成本地验证

两项 Flutter 回归证明旧版连续刷新发送 100 项请求、活动连接变化未更新选择；新实现通过。恢复旧 worker 取消检查方式的隔离变体证明等待任务被取消后仍执行；修复通过。覆盖身份展示、session revision/kind、公开字段脱敏与 Microsoft /me 请求。

根与独立 Rust 默认套件、严格 Clippy/格式、Flutter analyze/测试及 Linux 原生编译通过；本机 loopback WebDAV IPC/真实隔离 FUSE 读写暂停恢复与备份冲突保护、本机签名 UDP/QUIC 连接切换通过。最终原生 Linux/Windows 桌面 CI 均通过：Rust 65/59 项、Flutter 各 24 项，两平台实际 IPC 队列/取消/订阅兼容回归与 Windows 托盘烟测通过。托盘烟测模拟 Shell 通知，没有使用用户硬件输入或真实账号。[必需原生检查](https://github.com/LumiaBlack51/twodrive/actions/runs/38675644048)、[独立 WebDAV/QUIC](https://github.com/LumiaBlack51/twodrive/actions/runs/38675644035) 和 [Windows 综合预览](https://github.com/LumiaBlack51/twodrive/actions/runs/38675643947) 也均成功。

Linux tar 保持 CI 原字节；Windows ZIP 全部 35 个文件与 CI 原件逐字节相同、CRC 正常，三个 EXE 的 PE x64 头已核实。包中未包含 profiles.json/tokens.json/credentials.json；源码归档版本为 0.1.4。GitHub 6 项附件的状态、大小、SHA-256 和真实 tag 目标在公开前后由 verify-release-assets.py 校验通过。[verification.json](https://github.com/LumiaBlack51/twodrive/releases/download/dev-v0.1.4/verification.json) 和 SHA256SUMS 随包提供。

Linux 包 SHA-256：`02384eefc6a1dc5430ee7aa2d8b4b44d63d78015c12f39e6717bdb81811431fe`；Windows ZIP：`50baf3ad6ed5991ad1dc20cb598d9e20a38583ea3fb9b22ddea2193ac2b680a5`。

## 范围与升级

本轮未访问真实 OneDrive/Google 账号、未重新进行浏览器 OAuth，也未还原原生产截图的完整时序。OneDrive 展示探测失败时提示名称暂不可用，保持已有盘连接可用。轮询不是云端实时推送；取消等待当前网络 I/O 返回。Windows 文件浏览/传输不代表 FUSE 挂载。

退出 Development、备份独立状态与未同步缓存，再按包内 INSTALL.md 和 SHA256SUMS 升级；旧 session 在重新连接时补齐账号名称。稳定版服务/配置与稳定 Latest 不受这个预览版影响。本轮没有生产安装或服务恢复操作。

故障记录：[TD-20261015-03](incidents.md#td-20261015-03桌面状态轮询堆积与连接身份展示不足)。规范：[1.1](provider-integration-spec.md)。

English: Explicit sign-in/switch actions, an active-connection marker and adapter-provided account names now appear throughout the desktop and native tray tooltip. Single-flight polling and operation completion events recover state after long work; queued cancelled operations are skipped. Local/loopback evidence is separate from live cloud acceptance. Verify package checksums and preserve pending local data during upgrades.
