# 本地工作区清理记录（2026-10-07）

本次按用户要求清理已无用途的本地构建产物和重复工作区，不修改应用实现、安装或同步状态。开始前阅读了 `AGENTS.md`、`CONTRIBUTING.md`、[故障记录](incidents.md)、重构交付记录与 peer 调查记录。未发现或修复新的软件故障，因此不新增故障条目；原有未提交故障调查保持原样。

## 判断依据

`~/software/twodrive`、`~/software/twodrive-peer` 和 `~/software/twodrive-refactor` 共享一个 Git 仓库。后两者是历史开发工作区，不是独立安装的应用。

清理前重新获取并核实远端：

| 工作区 | 分支与已核实提交 | 判断与处理 |
| --- | --- | --- |
| twodrive | `codex/pin-download-feedback`，`7b50defc1eef074491b6d09a8e715e29e78677dd` | 已推送，但尚未包含在 `origin/main`；保留当前分支、源码和最新修复包。 |
| twodrive-peer | `codex/peer-control`，`dd0686bf23404abe4649408adaef60cfaa3a5333` | 工作区干净，本地与远端分支完全一致，提交已包含在 `origin/main` 和 `origin/dev`；移除重复工作区。 |
| twodrive-refactor | `codex/provider-boundaries`，`10bab5f12ea8f8f8b12279fc11e0a3550bab7348` | 工作区干净，本地与远端分支完全一致，提交已包含在 `origin/main` 和 `origin/dev`；移除重复工作区。 |

核实时 `origin/main` 为 `258438eb230409cc30d600c722f434f2e08bb3bc`，`origin/dev` 为 `355287f8a6d60883a4df18056c9aab1d7f5f9aca`。旧工作区中的实现仍有用途，清除的是它们的重复检出与构建缓存；本地分支、远端分支、标签与 Git 历史全部保留。现有 `~/.codex/worktrees/dev/twodrive` 也保留。没有切换主工作区分支或修改本地 `main` 指针。

删除前检查了两个旧工作区的全部忽略文件，范围仅为构建产物、`dist` 和 Python 字节码；没有发现额外运行配置或身份文件。待删除路径没有嵌套挂载，也没有观测到进程以其为工作目录、执行其中的二进制或打开其中的文件。

## 清理范围

- 清空主工作区的 Cargo `target`，包括 debug/release 依赖、中间文件、增量编译缓存和本地可重建二进制。
- 用 `git worktree remove` 移除 `twodrive-peer` 和 `twodrive-refactor`，包含它们的旧 `target`。没有使用强制删除。
- 删除主目录 `dist` 中 0.2.4-1、0.2.5-1、0.2.6-1、0.2.7-1、0.2.7-2、0.2.8-1、0.2.9-1、0.2.10-1 的 deb 与校验文件，共 16 个文件、50,692,231 字节。删除前，每个文件的大小与 SHA-256 都与 GitHub Release asset 元数据逐项一致；需要时可从相应 Release 下载。
- 随 peer 旧工作区移除 0.1.0 的解包副本和分发包；其正式实验产物仍在 [peer-v0.1.0 Release](https://github.com/LumiaBlack51/twodrive/releases/tag/peer-v0.1.0)。
- 删除主工作区两处 `__pycache__`。
- `git worktree prune` 清除已经不存在的 `/tmp/twodrive-release-0.2.8-1` 登记；没有删除其他有效工作区。

三个目录原先占用 21,549,740,032 字节。清理后主目录约 48,472,064 字节（写入本记录前），释放 21,501,267,968 字节，即 **20.02 GiB**。这是 `du -sxB1` 的目录占用差，不等于期间整个文件系统可用空间的变化。

## 集中保留的产物与资料

现有 `dist` 保留：

| 路径 | 用途 |
| --- | --- |
| `dist/twodrive_0.2.11-2_amd64.deb` 与 `.sha256` | 当前 pin 反馈修复包，已随修复分支提交；保持原字节。 |
| `dist/twodrive_0.2.11-1_amd64.deb` 与 `.sha256` | 最新正式发布的 Linux 包。 |
| `dist/rollback-0.2.10-2/` | 从 refactor 工作区复制的当前安装版本回退包、精确源码归档和 `SHA256SUMS`。 |
| `dist/peer-0.1.1/linux-x86_64/twodrive-peer` | 尚未创建正式 Release 的 Linux peer 0.1.1。 |
| `dist/peer-0.1.1/twodrive-peer-0.1.1-windows-x86_64.zip` | Windows peer 0.1.1 手动替换包；完整包含原解包目录的 exe、说明、构建记录、公钥和校验文件。 |
| `dist/peer-0.1.1/verification.json` | 原 peer 构建提交与摘要记录。 |

复制后逐文件校验一致，并核对 Windows ZIP 内全部五个文件与旧解包目录逐字节相同，因此不另保留重复解包副本。

重要摘要：

| 产物 | SHA-256 |
| --- | --- |
| 0.2.11-2 deb | `e2da4946333cdecdd7f67cb7b1481acb8de93a0675c807cd0a682f7c412a230f` |
| 0.2.10-2 回退 deb | `0301ea58cf2dc266bedc09dd115b765d91b3f05ff36b97da1945f4de62ae0374` |
| peer 0.1.1 Linux 二进制 | `8f03fba31b94a3a59f7f58e7e3466b4a07d88f6e91be49775319b21057786d2e` |
| peer 0.1.1 Windows ZIP | `c50ec246e014018964182a75546fbe9bf42b8c06363554596fa4bc6829b3ec30` |

`dist` 中此次集中保存的未跟踪产物仍遵循现有忽略规则，不因此进入 Git；尤其 peer 0.1.1 的本地交付包需要保留，不应仅凭源码已 push 就删除。

下列内容缺乏“已无用途”的充分证据，保持原位置和原字节：

- 未提交的 `TwoDrive-relay-lab/` 原始实验参考代码。peer 调查说明它是参考实现，但并不等于整份实验源码已在 Git 中备份或所有功能均已替代。
- 未提交的 `doc/incidents.md` 补记、Nautilus 监测说明和监测脚本。监测用户服务仍在运行。
- 历史调查、回归测试、架构文档、构建脚本与锁文件。

## 验证结果与边界

- 写入维护文档前，主工作区原有 146 个已跟踪和未跟踪文件均与清理前 SHA-256 一致，包括故障记录与原始实验源码。
- 两个被移除工作区分别在临时目录从其原提交重新检出，144 和 124 个已跟踪文件逐项校验一致；工作区状态干净。两个恢复副本的 `cargo metadata --locked --offline --no-deps` 均成功，随后移除验证副本。
- 主工作区清理前后的 `cargo metadata --locked --offline --no-deps` 均成功；`git diff --check` 通过，工作区登记再次核对无失效条目。
- 五个已安装 CLI、daemon、托盘、Settings 和 Nautilus 扩展文件摘要均未变化。CLI 报告 `twodrive 0.2.10`；集中保存的 Linux peer 报告 `twodrive-peer 0.1.1`。
- TwoDrive 服务保持 `MainPID=3051`、`active/running`、`NRestarts=0`；监测服务保持 `MainPID=3040`、`active`；原 OneDrive 路径仍为 FUSE 挂载。
- 本次不涉及应用代码修复，没有执行新的 Rust 编译、完整回归、GUI 操作或真实 OneDrive 上传下载。上述证据确认清理保留了源码、安装文件、进程和挂载，并证明旧工作区可恢复；不扩大历史故障的验证结论。

## 日后开发与恢复

构建缓存可从保留的源码与 `Cargo.lock` 重新生成；下一次完整编译需要重新编译依赖，Cargo 全局下载缓存没有清理。主工作区仍停留在 pin 反馈修复分支。

继续修改主工作区时，按 [Contributing](../CONTRIBUTING.md#build-and-check) 构建，例如：

```bash
cd ~/software/twodrive
cargo build --workspace --locked
```

如需恢复旧分支专用工作区，执行：

```bash
cd ~/software/twodrive
git worktree add ../twodrive-peer codex/peer-control
git worktree add ../twodrive-refactor codex/provider-boundaries
```

如需从最新远端主线开始新的修改，先 fetch，再创建新的开发分支和工作区；不要用旧 peer/refactor 分支代替当前主线。当前已存在的 `dev` 工作区也可继续使用。

以后清理时先核对 `git status`、提交的远端备份、忽略文件、运行进程路径和仅存本地的产物。只需清除当前工作区可重建产物时可运行 `cargo clean`；保留 `dist` 的待发布包与所需回退包。不要直接对整个仓库执行包含忽略文件的批量删除。

本次仅本地清理和维护文档更新，未提交、推送、合并、发布或安装新内容。

## 2026-10-13 留档补记

本记录描述2026-10-07的清理事实；上文“尚未提交”和当时的分支/进程状态不代表当前状态。用户现已要求提交遗漏资料并合入 dev。清理记录、Nautilus调查和诊断脚本现一起纳入版本管理，原始 relay-lab 的15个文件保持原字节归档于 [TwoDrive-relay-lab](../TwoDrive-relay-lab/README.md)，另加归档用途与已知边界说明。实验源码不接入工作区成员、桌面、daemon 或 peer，历史发布脚本和 CI 模板未执行/启用。

本次提交留档没有再次清理构建产物或回退包，也没有更新安装、服务、挂载和用户运行状态。当前功能代码及发布交付见 [2026-10-13提供者记录](provider-boundaries-2026-10-13.md)；原有清理证据和验证边界保留。已提交 [34dbb83](https://github.com/LumiaBlack51/twodrive/commit/34dbb838ca623a95f2401a7e9777d307e0c040a2) 并通过 [PR #12](https://github.com/LumiaBlack51/twodrive/pull/12) 合入 dev，合并提交为 [2e9e523](https://github.com/LumiaBlack51/twodrive/commit/2e9e5239a77f8c84c29a70542576e0556eebac98)。
