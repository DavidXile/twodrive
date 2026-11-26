# 0.1.5 发布后的工作区清理（2026-10-15）

先完成 [Development 0.1.5](https://github.com/LumiaBlack51/twodrive/releases/tag/dev-v0.1.5) 发布和 15 项附件公开前/后校验，再执行本轮清理。发布于 16:36:50 +08:00；标签源码为 `98c82aa0b9847b66cd199a012abf14b3ddb3aefa`，发布源五组 CI 全部通过。MSIX 和 Deb 的最终 CI 产物都再次通过实际安装验收，详见 Release 的 runtime-validation.json / verification.json。

## 已清理

- 已合入 dev、工作树无改动的 distribution-dev 临时 checkout，连同 Cargo target、Flutter build/ephemeral、旧 CI 解包、MSIX、下载及日志；其提交和代码仍在 Git 历史。
- 活动源码目录的 Flutter/Dart 生成状态、构建和 Python 字节码。
- 旧 Windows 原生源码镜像、三套重复预览产物、已发布旧 Release 的本地下载副本、旧源码 ZIP/bundle 和一次性脚本/日志。旧 5489c802 源码已核实属于远端 dev 历史；镜像的 188 个文件可从远端 Git 恢复。
- 原来被跟踪的 dist/twodrive_0.2.11-2_amd64.deb 及校验文件。当前打包使用构建输出；删除旧二进制可避免继续纳入未来源码归档。
- WSL 的本轮 Ubuntu Base 验收根目录，逻辑大小 914,726,870 字节。安装测试使用隔离 namespace，FUSE 已卸载，测试 daemon 已退出。

项目目录内删除文件的逻辑大小约 **12.53 GB**；首批清理时 E: 磁盘可用空间实测增加约 **10.62 GB**。后者是磁盘观测，不把硬链接重复统计或 WSL 虚拟磁盘文件大小当成已回收的宿主磁盘空间。

## 保留

- p2p-dev 原有 7 个未提交文件；逐文件 SHA-256 清理前后一致，原分支/公共 Git 数据保留。
- p2p-test-20261011 内仍运行的 Windows 客户端及其代码/配对状态；只移除不用的压缩下载。
- windows-preview-state 和 ui-verification 的持久状态，避免丢弃可能未同步的数据。
- 固定版本 Flutter 3.41.8 SDK，作为本项目锁定版本开发工具。
- 未发布的历史调查/设计说明及真实截图，整理到 local-notes；这些本地记录没有作为新代码推送。

## 尚余的受阻清理

`release-work-20261015` 暂存目录仍存在，约 **282.8 MB**。整目录删除及更窄的逐文件清理均被工具自动审批拒绝，返回原因只有 `blocked by policy`；未继续绕过该限制。因此不能宣称本轮已删除所有暂存文件。具体清单与哈希保存在本地 CLEANUP-20261015.json。

本次产品运行逻辑没有变化，故障与验证边界沿用 [0.1.5 安装验证](development-0.1.5.md)。
