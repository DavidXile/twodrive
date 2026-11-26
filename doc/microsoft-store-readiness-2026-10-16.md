# Microsoft Store 发行评估（2026-10-16）

评估基线：`dev` [`bb831776abc086734febec3607c6e55da52de1eb`](https://github.com/LumiaBlack51/twodrive/commit/bb831776abc086734febec3607c6e55da52de1eb)。
结论：现有 MSIX 是可继续推进的安装工程基础，**尚不是可直接正式提交的产品**。
最近的路线是把现有 Development 桌面按已实现的文件传输、备份和设备共享范围收口，
完成商店身份、资料和原生包验收。若商店首版承诺 Windows 文件按需访问，CFAPI
是必须另行实现和验收的功能；更换打包格式不能补齐它。

## 已经验证什么

已读取清单、`scripts/build-msix.ps1`、桌面及 Full/Lite CI、运行时脚本、
[PR #33](https://github.com/LumiaBlack51/twodrive/pull/33)、
[PR #34](https://github.com/LumiaBlack51/twodrive/pull/34) 及
[0.1.6 runtime-validation.json](https://github.com/LumiaBlack51/twodrive/releases/download/dev-v0.1.6/runtime-validation.json)。
两个 PR 的讨论中没有额外的人工商店认证结论；Copilot 的额度提示不是代码审核通过。

- 0.1.6 应用/打包输入 `e64bd76485c225c460c8189f54477a9e22728a3f`；
  验收工具提交 `2bccf2b2e30123ae22c4e1c2b41a57e8a79500bb`。
- Windows 11 原生安装成功；AUMID 激活的 GUI 及子引擎确有包身份；
  MSIX 更新标记存在；退出消息循环后引擎回收；卸载及依赖注册恢复通过。
- 包调试上下文中的隔离 IPC/本机 QUIC 覆盖配对、传输、撤销、重连、只读权限和退出。
  退出通过 WM_QUIT 注入；调试上下文通过 `Invoke-CommandInDesktopPackage` 建立。
  **不等于实际鼠标菜单、完整 GUI、浏览器 OAuth、真实云盘或异地 NAT 验收。**
- 最终测试包 SHA-256 为
  `e32fbdfb9fafd43b84d1a7bc8ba9b9d34c9cd0f2fa40918aa5d7a77fc82b8ca7`。
  报告明确 `signed=false`、无证书信任变更、无商店认证。

## 当前到底是哪一个 Windows 产品

| 路径 | 已实现范围 | 商店结论 |
| --- | --- | --- |
| 本 MSIX：`apps/dav` → `twodrive_dav.exe`，附 `twodrive-dav-engine.exe` / `twodrive-dev.exe` | Development 管理界面、提供者连接、显式上传下载、备份、共享；Linux 另有 FUSE | 可以作为有明确功能边界的首版候选，仍须以下验收 |
| 旧 Full/Lite：`apps/full` / `twodrive-windows` | OneDrive 登录、目录/持久索引、显式只读下载/续传、缓存；独立引擎/托盘 | 不是当前 MSIX 的内容；不能把它的历史结果替代 Development 验收 |
| Windows Explorer 文件按需 | CFAPI 同步根、占位文件、范围取数、云状态图标、双向变更闭环 | 两条路径均未完成；不能宣传成 OneDrive 式文件按需客户端 |

依据：`doc/windows-acceptance.md` 的原生同步门槛仍未通过；
`experiments/twodrive-dev/src/desktop.rs::capabilities` 仅在 Linux + mount 特性下提供挂载。
Full GUI 存在不代表同步引擎已完成，也不代表全部界面已做用户操作验收。
目前 Google Drive 还要求用户自备 Desktop OAuth JSON；如果首版继续提供，必须在商店说明中
披露这一前提并提供可复现的测试步骤，不能宣传为普通用户开箱即用的 Google 登录。

## 三类缺口和现状

| 分类 | 项目 | 现状与下一步 |
| --- | --- | --- |
| 可通过代码解决 | 测试/商店身份隔离 | 本次修复：非 `-TestPackage` 路径拒绝保留测试 Name 和 unsigned OID，防止产出误标为 store 的测试身份包 |
| 可通过代码解决 | 语言声明 | 本次修复：界面主要为中文，清单改为仅 `zh-cn`；完成英语 UI 后再恢复英语声明 |
| 可通过代码解决 | 第三方许可证 | 打包只显式复制项目 LICENSE；Flutter 自带 notices 不能证明 Rust、原生插件等全体许可义务已覆盖。按锁文件和最终 payload 补齐/核验第三方 notices，保留可读入口 |
| 可通过代码解决 | Windows 秘密存储 | OneDrive/Google token 使用 core 的 DPAPI；WebDAV profile 的 password 和设备状态另走 JSON State，不能泛称全部凭据已加密。提交前核查 Windows ACL/静态保护；若保护不足，补充兼容迁移和回归。本次未做跨用户访问测试，不据此预判认证结果 |
| 可通过代码解决 | 可选自启动 | MSIX GUI 已禁用便携版开关，清单无 StartupTask。首版可以不承诺自启动；若承诺，需使用受支持 StartupTask 并独立测试，不能写入带版本的 WindowsApps 路径 |
| 可通过代码解决 | 隐私与商店资料 | 编写与实际数据流一致的政策和产品说明，明确云服务、文件内容/元数据、授权、IP/发现/中继、用户选定的备份/共享目录、撤销与删除；需发布者确认联系方式和最终文本 |
| 需要你提供账号资料 | Partner Center | 注册并通过身份核验、预留名称、创建 MSIX 产品；提供 Product identity 三个字段，不能从 GitHub 用户名推导 |
| 需要你提供账号资料 | 正式产品决策 | 确认发行主体、预留名称、首版范围、价格/市场、IARC 年龄问卷、政策地址、支持联系方式和认证测试入口；测试账户只交给认证人员，不提交仓库 |
| 需要你提供账号资料 | 签名边界（无需证书） | **商店 MSIX 提交无需购买 CA 证书、PFX/CER 或硬件令牌。** 认证后由商店重新签名。商店外正式侧载才另需合适的签名分发方案；本任务不生成或信任测试证书 |
| 需要真实设备测试 | 干净安装/运行依赖 | 无开发工具、无预装 VC 运行库的普通用户环境；确认 Store 自动部署 Desktop VCLibs，Flutter/插件/Rust 子进程完整，无 Python/Rust/Flutter SDK 运行时要求 |
| 需要真实设备测试 | 包内真实功能 | 新用户启动、真实 OneDrive/Google OAuth、取消/刷新/退出登录、文件选择器、读写/上传下载/改名删除、备份与错误恢复；只操作专用测试数据 |
| 需要真实设备测试 | 托盘与网络 | 鼠标退出/后台/重启、DPI/多屏、键盘/可访问性、Explorer 重启；普通用户防火墙提示、局域网及异地双机共享、代理关闭后的直连/中继实测 |
| 需要真实设备测试 | 升级/卸载与数据 | 同一正式 Name/Publisher 的 A→B 升级，登录/设置/未完成上传/本地脏数据保留；运行中更新/卸载无遗留进程，用户文件不被删；检查状态、秘密、共享入口及防火墙残留 |
| 需要真实设备测试 | WACK/商店分发 | 用当前 Windows SDK 的 WACK 保存完整报告，处理适用失败；再验证 Store 签名的私有分发版本及商店更新。CI MakeAppx 不能替代这些步骤 |

没有账号并不妨碍继续代码、文档和测试；但不能核实尚未提供的 Product identity
是否属于发布者，也不能声称 Partner Center 或商店认证已通过。

## 清单与发行规则核对

- **身份**：测试 Name 为 `TwoDrive.Development.Test`；其 Publisher 的特殊 OID
  专供 Windows 11 `-AllowUnsigned` 测试。正式 Name、Publisher、PublisherDisplayName
  必须逐字匹配 Partner Center，包括大小写、标点。显示名目前固定为
  `TwoDrive Development`；预留其他名称时须同步调整两处显示名，不能只换 Identity。
- **版本**：当前 `0.1.6` 映射 `1.1.6.0`。脚本已限制主段非 0、各段不超过
  65535、第四段为 0；正式更新选择高于同身份已发布版本的版本号。
- **权限**：仅 `runFullTrust`，与 Flutter/Win32 子进程和用户选定的本地文件操作相符。
  在 Partner Center 填写实际用途并完成所要求的 restricted-capability 审查；
  不应为了网络访问给 medium-IL Win32 程序乱加 AppContainer 权限，
  也不需要因文件选择器盲目加 `broadFileSystemAccess` 或 `allowElevation`。
  full trust 不等于管理员权限。当前没有 CFAPI/COM/文件关联扩展，不能当作功能已经存在。
- **依赖**：x64、Windows.Desktop、最低 `10.0.19041.0`、MaxVersionTested `10.0.26100.0`；
  `Microsoft.VCLibs.140.00.UWPDesktop >= 14.0.30704.0` 已声明。
  商店可部署框架依赖；现有一台 Windows 11 安装结果不足以证明最低 Windows 10
  系统及全部声明目标。测试最低系统，否则收紧最低版本；ARM64/x86 原生包不是首版必需项。
- **更新**：`distribution-msix` 已让客户端停止 GitHub 更新检查；打包已移除便携安装器。
  这证明了更新渠道选择，不证明 Store 更新、状态迁移、卸载清理已验证。
  测试包和正式包是不同身份，不能把 `-AllowUnsigned` 包的升级当成正式 Store 升级。
- **材料**：至少一个真实截图、准确描述、分类、市场/定价和年龄评级问卷；
  本产品需要隐私政策。支持联系方式建议提供；PC 的支持字段并非普遍强制。
  图标和宣传图以实际尺寸要求准备，现有三个包内图标不是全部商店素材。
  认证备注提供干净账号/服务器的使用路径、所需外部服务/凭据、托盘及后台退出步骤。
  `.msix` 本身可提交；`.msixupload`、多架构 bundle、符号有用，但不是本 x64 首版的必备重构。

## 最短执行顺序

1. **确定首版边界**：以当前 Development 文件传输/备份/共享工具收口，不承诺
   Explorer 文件按需。若文件按需是首版硬性目标，先完成既有 CFAPI 验收清单再提交。
2. **完成发布者账号**：从 [Store developer](https://storedeveloper.microsoft.com/)
   进入当前注册流程。微软当前文档说明个人和公司均免注册费；仍需身份/主体核验。
   预留名称后，在 Product management → Product identity 复制三项公开身份值。
   提供这些值即可准备正式身份包，**不要发送 Microsoft 密码或证书私钥**。
3. **收口现有版本**：完成第三方许可、秘密保护审计、政策/截图/认证说明；
   确认 Google 自备凭据边界、中文界面和不支持的 Windows 挂载均清楚。
4. **从同一次成功 Windows CI 的完整 bundle 构建正式身份候选**。已有脚本可用：

   ```powershell
   ./scripts/build-msix.ps1 -Bundle <完整Windows-bundle目录> `
     -IdentityName '<Partner Center的Package/Identity/Name>' `
     -Publisher '<Partner Center的Package/Identity/Publisher>' `
     -PublisherDisplayName '<Partner Center的PublisherDisplayName>' `
     -Version '<本次四段版本，例如首次可用的1.1.6.0>'
   ```

   结果仍为 unsigned submission candidate，**不是普通侧载安装器**；不对正式身份使用
   `-AllowUnsigned`，也不向正式 Publisher 添加测试 OID。保留源码 SHA、包 SHA-256 和 CI 链接。
5. **设备/WACK验收并提交认证**：在专用测试机执行上表矩阵，保留日志、系统版本和真实截图。
   可先用保留测试身份验证同 payload 的行为；该身份的 WACK/运行结果必须如实标明身份差异。
   在 Partner Center 先准备私有受众或包航班（按账号实际可用选项），完成认证后验证
   Store 签名分发包及 A→B 更新，再切换公开发布。若认证前必须侧载正式身份，
   另制定合法签名测试方案，不冒用测试证书代表商店签名。

## 本次代码与验证范围

本次仅收紧 MSIX 身份输入和语言声明，并新增原生 Windows 打包回归，未修改同步架构、
稳定版 main、凭据格式或签名策略。测试使用临时的合成 Store identity 跑 MakeAppx
分支；临时包不安装、不上传，也不证明真实商店身份已获授权。
回归核对两种模式的清单、版本、语言、未签名状态、更新标记、移除便携安装器、
SHA-256 和保留 payload 的逐字节一致性。

Linux 主机只能执行 Python 语法/发现，原生三项明确跳过；原生执行结果以本 PR Windows CI 为准。
实机、WACK、正式身份和认证未在本次执行。故障条目见
[TD-20261016-01/02/03](incidents.md)。

2026-10-16 CI 补记：验证源码
[`432e35ca0962ebed3d81e7cececf810587699ce2`](https://github.com/LumiaBlack51/twodrive/commit/432e35ca0962ebed3d81e7cececf810587699ce2)。
[桌面 CI](https://github.com/LumiaBlack51/twodrive/actions/runs/38864468289) 的 Linux 检查通过；
Windows 的 Rust/IPC、Flutter/托盘、三项原生 MSIX 回归及正式测试包生成/上传均通过。
两种模式的内容断言在真实 Windows SDK 下执行，不是 Linux 跳过或静态文本检查。
首轮 Python → PowerShell 5.1 的模块路径继承导致 hash 命令缺失，修复仅限测试子进程环境；
失败及修正证据保存在 TD-20261016-03。此证据不包含普通用户 Windows 客户端、
真实 Partner Center 身份、Store 签名分发、升级、WACK 或认证。

## 微软官方依据（2026-10-16 核对）

- [包格式、商店签名与版本](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/app-package-requirements)
- [Product identity 三个字段](https://learn.microsoft.com/en-us/windows/apps/publish/view-app-identity-details)
- [保留的 unsigned 测试身份](https://learn.microsoft.com/en-us/windows/msix/package/unsigned-package)
- [注册流程与费用](https://learn.microsoft.com/en-us/windows/apps/publish/partner-center/open-a-developer-account)
- [能力声明与 full trust](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/app-capability-declarations)
- [提交材料、年龄评级及认证选项](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/create-app-submission)
- [隐私与支持信息](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/support-info)
- [现行政策：10.1 功能、10.2 安装、10.5 隐私、10.6 权限、10.7 本地化](https://learn.microsoft.com/en-us/windows/apps/publish/store-policies)
- [框架依赖](https://learn.microsoft.com/en-us/windows/msix/msix-troubleshooting-guide)
- [Windows App Certification Kit](https://learn.microsoft.com/en-us/windows/uwp/debug-test-perf/windows-app-certification-kit)
