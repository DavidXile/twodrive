# Development 0.1.5：共享 GUI、更新检查与发行准备

2026-10-15（Asia/Shanghai）。版本为 `0.1.5` / Flutter `0.1.5+6`，MSIX 映射为 `1.1.5.0`。发布目标为 [dev-v0.1.5](https://github.com/LumiaBlack51/twodrive/releases/tag/dev-v0.1.5)；实际公开状态、不可变标签源、各附件大小及 SHA-256 以该 Release 的 `verification.json` / `SHA256SUMS.txt` 为准。稳定版与 Development 使用不同版本序列。

## 本轮内容

- 服务端 GUI：在“共享”中选择目录、启停服务、导出连接文件、撤销设备；默认只读，关闭窗口留在托盘，退出停止共享。客户端“添加连接”可直接导入并保存连接。
- Development 自动更新检查：后台节流、可关闭与手动检查，提示发布链接；不自动安装。MSIX 使用分发通道标记。
- Windows 配置原子替换短时占用时有界重试，永久错误保留旧内容并上报。
- MSIX 开发测试身份采用微软规定的 unsigned OID，Windows 11 可显式 `Add-AppxPackage -AllowUnsigned` 安装，无需导入测试根证书。正式签名/商店身份不使用该 OID。
- Debian 附件是已有 **GNOME OneDrive 客户端 0.2.14-2**，不是 Flutter Development GUI 的 deb。debhelper、自动依赖、版权/手册、源码包与完整上传清单已准备；官方 archive 依赖闭包与审核仍未完成。

## 实际安装验证

Windows 11 本机：从完整 `0.1.5` bundle 构建 MSIX 后真实安装，通过 AUMID 激活 GUI，分别用系统 API 断言 GUI 与真实引擎子进程拥有安装包身份，核对分发标记，并验证 GUI 退出后引擎结束。再用微软的 `Invoke-CommandInDesktopPackage -PreventBreakaway` 在包调试上下文中执行既有真实 IPC/QUIC 六组回归：配对、上传/下载字节、撤销、同引擎客户端/服务端共存、重启/退出、权限轮换。最后卸载，并核对测试包注册和依赖注册恢复。没有修改证书信任；调试上下文不等同于真实 GUI 的全部交互测试。

复验命令（管理员 Windows 11；依赖仅接受有效 Microsoft 签名的 Desktop VCLibs APPX；脚本拒绝替换已有同名测试包）：

```powershell
./scripts/test-msix-runtime.ps1 -Package <TEST_UNSIGNED_MSIX> -Report <NEW_REPORT_JSON> -VcRuntime <MICROSOFT_VCLIBS_APPX>
```

Debian 格式包：在 WSL2 内使用经官方 SHA-256 校验的 Ubuntu Base 24.04 根文件系统，bubblewrap 隔离文件系统、PID/IPC/UTS，安装实际 deb 及声明依赖。文件清单、两份上传清单、CLI/动态 SQLite、GTK 设置进程在 Xvfb 下启动通过。使用包安装到 `/usr/bin` 的 CLI/daemon 完成真实隔离 FUSE hydration、写入、上传确认、移动、删除、pin/unpin/release，再卸载确认命令移除。数据后端为 disposable mock，不能称为真实 OneDrive 验证。宿主 WSL 的应用和服务未安装/替换。

机器可读本机结果：[runtime-validation-0.1.5.json](runtime-validation-0.1.5.json)。最终版本 CI 运行与最终附件校验结果随 Release 保存；Linux/Windows 桌面、原生检查、QUIC、Windows 预览和 Debian 构建都必须成功后才公开。

## 范围与清理

普通 Windows 用户使用便携 ZIP。未签名 MSIX 是开发测试/商店准备资产，不能作为通用可信安装程序；正式签名、商店认证、原生文件选择器逐项交互、新浏览器 OAuth、异地 NAT 和 Debian 官方入库仍未验证。清理只在发布并校验远端附件后进行；保留唯一未提交代码、在用客户端和持久配置，移除可重建目录、重复下载与已合并的临时 checkout。

安装验收中的 fixture 问题与修正见 [TD-20261015-11/12](incidents.md)。

参考：[微软 unsigned MSIX](https://learn.microsoft.com/en-us/windows/msix/package/unsigned-package)、[包上下文调试接口及其边界](https://learn.microsoft.com/en-us/powershell/module/appx/invoke-commandindesktoppackage)。

English: Development 0.1.5 adds desktop hosting/export/import and update checks. Native Windows MSIX install, real GUI/engine identity, package-context IPC/QUIC, exit and uninstall were verified without changing certificate trust. The separate GNOME deb passed isolated Ubuntu installation, GTK startup and installed-binary mock FUSE I/O/removal. Neither result establishes live cloud, independent-network NAT, Store certification or official Debian acceptance. Exact final-source CI and asset provenance are recorded with the release.
