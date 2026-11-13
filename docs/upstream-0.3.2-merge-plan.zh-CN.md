# 官方 0.3.2 合并记录与后续同步说明

## 实际合并结果（2026-11-13）

已在 `codex/upstream-0.3.2` 分支完成源码集成和验证，以原 fork 最新提交 `01ac361829103185f3df5b428bc0892e5beb38b5` 为第一父提交，合入官方 `fef87ac`（包含指定的 `935b1d8d07747433f401c7fbd4e875a9f2f0041a` 及其后 README 更新）。采用真正的 merge commit，保留双方祖先关系，方便以后继续同步；本次不推送、不打标签、不发布。

- 已引入：会话视图、邮件规则、本机验证码识别和复制、AI 自定义要求、三种邮件代理模式、悬浮提示及写信窗口交互改进。
- 保留 fork MCP：本机 HTTP 服务、独立 stdio 代理、全局和逐账号的读/写/发权限、桌面确认及独立设置页；未接入官方 `--mcp` 启动入口、`exePath`、AI 页 MCP 配置或官方只读服务。
- 保留截至 `01ac361` 的全部本地提交，包括统一正文/签名/引用渲染、签名折叠、Junk 移动与回滚、草稿局部编辑、MCP 设置简化、更新弹窗和 Windows 安装/卸载钩子。
- 应用、MCP 代理及相关锁文件统一为 `0.3.2`；未引入官方前端 `marked` 渲染，继续使用 fork 的后端统一渲染。
- 数据库沿用 fork v3 草稿迁移，追加 v4 会话、v5 规则。对官方 v3/v4 来源按实际列和表补齐草稿结构，不重建已有会话编号或规则。升级前保留原有备份机制；数据库升到 v5 后，旧程序不能直接打开该库。

验证结果：

| 检查 | 结果 |
|---|---|
| `npm run build` | TypeScript 与 Vite 构建通过 |
| `npm test` | 10 项通过 |
| `cargo test --locked --workspace --manifest-path src-tauri/Cargo.toml` | 126 项通过，1 项原有真实 OAuth 测试跳过 |
| `git diff --check HEAD` | 通过 |
| fork MCP 核心、更新弹窗和安装钩子与原 HEAD 比较 | 核心文件保持原样；共享 `mcp_tools.rs` 仅补会话参数默认值 |

新增回归测试覆盖 fork v3 草稿、MCP 授权、AI 批注及待办的升级保留；官方 v3/v4 来源的补齐与重复打开；规则移到 Junk 时复用现有队列、保留正文/会话、阻止重复排队并可失败回滚。测试使用临时数据和内存通道；未启动真实邮箱端到端流程、未实际发信，也未验证真实 Agent 客户端和 macOS 安装包。

### 下次同步需要延续的约定

1. 从包含本次 merge commit 的 fork 分支继续合入官方后续提交；上游同步保留 merge ancestry，不把这次同步压成一个无上游父节点的 squash 提交。
2. 官方与 fork 的 MCP 方案不同。仅独立实现文件可保留 fork 版本；`commands.rs`、`settings.rs`、`store.rs`、`lib.rs`、前端 store 和 Settings 等共享文件必须逐块审阅，不能整份选择 ours。
3. fork 当前数据库最新版本为 5，官方本次为 4。以后迁移只能在 fork 迁移数组末尾追加，先比较含义和实际结构，不能照搬官方编号或更改 fork 已发布迁移。
4. 继续执行构建、前端测试和 Rust workspace 测试；涉及共享权限、草稿、规则、会话及移动时，验证相关组合行为。
5. 当前 updater 仍使用官方 endpoint 和公钥。本次只合并功能；fork 的更新来源、签名及标签发布流水线留待后续 GitHub 流程任务统一处理。已确认目标为 Windows x64、macOS arm64/x64，全部构建成功后自动公开 Release，目前尚未实现。

以下保留分析阶段的功能明细和三方预演，作为本次取舍依据；其中仓库状态、预演和建议命令均是实施前的历史记录，以本节实际结果为准。

## 合并前分析（历史记录）

分析日期：2026-11-13，Asia/Singapore。

目标：以自己的 fork 为基础，引入官方 `935b1d8d07747433f401c7fbd4e875a9f2f0041a` 的非 MCP 功能，继续使用 fork 现有的 MCP 服务、授权、代理和桌面确认方案。

分析阶段仅拉取官方 Git 对象、阅读源码，并在临时目录做逐文件三方合并预演。后续实际集成结果见文首。

## 1. 比较范围与仓库状态

- `origin`：`https://github.com/ekoouu/kojoomail.git`，官方仓库。
- `fork`：`https://github.com/iamsxm/kojoomail.git`，自己的仓库。
- 当前分支：`ci/mcp-p2`。
- 共同祖先：`31361a644c43e98d7ee098671ed7ddc0ac43c422`，官方 0.3.1。
- 指定提交：`935b1d8`，官方 0.3.2，提交时间折合新加坡时间为 **2026-11-13 11:39:20**。
- 拉取时官方 `main` 为 `fef87ac`：在指定提交之后只补充了 README，时间为 **2026-11-13 12:04:54**。
- 指定提交相对共同祖先：28 个文件，新增 2399 行、删除 71 行；全部功能集中在一个提交中，不能按功能直接挑选多个上游提交。
- 远程 fork 的 `main` 仍是 `31361a6`；远程 `ci/mcp-p2` 是 `73f18c1`。应基于自己的功能分支继续集成。

检查期间，本地分支继续产生新提交，并有未提交的更新弹窗、安装逻辑等修改。三方预演固定读取时的 HEAD 为 `3f7d2b88b4db8d6e6081f4c1c49d318e15f388bd`，并包含当时工作区文件；之后还观察到 `0c609a5`。实际合并前需要重新确认当前 HEAD 和工作区状态。

## 2. 官方新增了什么

| 功能 | 实际行为与限制 | 主要源码 |
|---|---|---|
| 会话视图 | 默认关闭；同账号内根据 Message-ID、In-Reply-To、References 归并，不按相同主题粗略合并。列表显示封数、参与人、未读等；阅读区按时间显示整串。回复、删除、归档等仍针对单封。搜索和重点、稍后处理、账单、待跟进仍按单封显示 | `thread.rs`、`store.rs`、`commands.rs`、`mailbox.rs`、前端 store/MailList/Reader |
| 邮件规则 | 设置中新增规则管理；按发件人、收件人、主题、有无附件匹配，支持全部/任一条件、顺序、命中后停止；动作包括移动、归档、垃圾邮件、已读/未读、重点/取消重点。仅对新同步到收件箱的信触发，首次同步、UIDVALIDITY 重建和本机移动触发的同步跳过；不会批量处理历史信，不提供自动删除或自动发信 | `rules.rs`、`Rules.tsx`、`mailbox.rs`、`store.rs` |
| 验证码识别 | 本机规则提取验证码，在通知前部显示，读信页提供复制。主题像验证码但缺正文时最多等正文 15 秒；不调用模型 | `text.rs`、`ai.rs`、`commands.rs`、`lib.rs`、`mailbox.rs`、`Reader.tsx` |
| 自定义 AI 要求 | 最多 500 字，影响完整批注、AI 起草回复和跟进邮件。已有批注需要重新生成；仅分类模式不追加该偏好；没有把偏好接到所有 AI 功能上 | `settings.rs`、`ai.rs`、`commands.rs`、`assist.rs`、`Settings.tsx` |
| 可选代理模式 | 跟随系统、不用代理、自定义 HTTP CONNECT 代理；不支持 SOCKS。自定义代理失败时报错，不自动直连；已建立的邮件连接在下次重连后采用新设置。改动作用于邮件连接，不能据此认为 AI HTTP 请求也切换到此代理 | `net.rs`、`settings.rs`、`lib.rs`、`Settings.tsx` |
| 界面细节 | 被截断文字的完整悬浮提示；写信弹层点击遮罩不关闭，可通过指定区域拖动窗口；批注按钮提示位置调整 | `ui.tsx`、`styles.css`、`Compose.tsx`、`Digest.tsx`、`MailList.tsx`、`Reader.tsx` |
| 官方 MCP | 主程序以 `--mcp` 启动，在独立进程中只读打开本地数据库，用 stdio JSON-RPC 暴露 4 个工具：list_accounts、search_mail、read_mail、mail_ai_notes。默认关闭，账号可读性受 ai_allowed 约束 | `mcp.rs` 及多个共享文件 |

0.3.2 的包版本号也同步更新。`fef87ac` 补充的 README 另外包含功能说明、路线图入口和贡献者致谢。

## 3. MCP 的保留与排除边界

fork 已有的实现是主应用内的本机 HTTP MCP 服务，加独立 stdio 代理；有令牌、全局和逐账号授权、读/写/发权限、草稿与确认后发送、桌面确认等逻辑。官方的只读独立进程实现不是对该方案的增量升级。

必须保留 fork 的：

- `src-tauri/src/mcp.rs`、`mcp_tools.rs`、`mcp_scope.rs`、`mcp_approval.rs`。
- `src-tauri/mcp-proxy/`、`src/lib/mcp.ts`、MCP 设置页面及构建/打包配置。
- `mcpEnabled/mcpRead/mcpWrite/mcpSend/mcpSendMode/mcpPort/mcpBodyMaxChars/mcpAllowAi` 和逐账号权限。
- `commands.rs`、`assist.rs` 等共享后端里的 scope 过滤、既有 MCP 命令及确认逻辑。

应排除官方新增的：

| 位置 | 排除的改动 |
|---|---|
| `src-tauri/src/mcp.rs` | 官方整个文件；保留 fork 同名文件 |
| `src-tauri/src/lib.rs` | `--mcp` 启动分支、`mcp::run(&data_dir())`；已有 `mod mcp` 只保留一个 |
| `src-tauri/src/commands.rs` | 专用于官方 MCP 配置的 `Boot.exe_path` 及赋值 |
| `src/lib/api.ts`、`src/store.ts` | 对应 `exePath` 类型、状态和 bootstrap 赋值 |
| `src/views/Settings.tsx` | AI 设置页中的官方 MCP 开关、`McpConfig()` 和调用；保留自己的独立 MCP 设置页 |
| `src-tauri/src/settings.rs` | 不新增第二份 `mcp_read`，不采用官方默认值或改写 fork 的授权含义 |

特别注意：官方 `mcpRead` 相当于整个只读通道的开关；fork 的整个服务开关是 `mcpEnabled`，`mcpRead` 是服务内部的读取权限，默认值也不同。字段重名不代表语义一致。

不能通过“跳过所有包含 MCP 的文件”完成集成：会话、验证码、AI 要求、代理都修改了这些共享文件。也不能把整个 `commands.rs`、`settings.rs`、`store.ts` 选成任一方的版本。

`store::LATEST` 改成 `pub` 是官方独立 MCP 读取数据库版本的需要；排除官方实现后无需仅为此修改可见性。

## 4. 必须先解决的数据库版本冲突

官方：

```text
v2 translations
v3 threads：messages.thread_id、thread_keys、thread_seq、旧邮件回填
v4 rules
```

fork 当前：

```text
v2 translations
v3 draft_content：drafts.content_json
```

对已有 fork 用户，正确的追加顺序应为：

```rust
const MIGRATIONS: &[Step] = &[
    v2_translations,
    v3_draft_content,
    v4_threads,
    v5_rules,
];
```

把官方函数重命名并追加，保留 fork 已经使用的 v3，最终数据库版本为 5。直接用官方迁移数组会使现有 fork v3 数据库跳过会话建表，之后查询 `thread_id` 失败。

上述顺序针对 fork 自身历史。若还需要兼容已经运行过官方 0.3.2 的数据库，仅靠版本号不够：官方 v3/v4 与 fork v3 含义不同，必须结合实际表/列检测补齐缺失结构，避免重复 ALTER，并为两种来源分别测试。不能宣称上述简单追加已支持两边数据库互换。

会话迁移会给旧邮件回填会话编号；首次运行需验证迁移备份与事务回滚。升级到 v5 后也不能直接用旧 fork 二进制打开该库。

## 5. 临时三方预演结果

使用共同祖先、当时工作区文件和官方指定提交，在临时目录调用 `git merge-file -p`；统一 CRLF/LF 后比较，没有写入仓库索引或产品文件。这是逐文件文本预演，不等于完整 Git 合并或功能测试。

| 结果 | 文件 |
|---|---|
| 同名新增冲突 | `src-tauri/src/mcp.rs` |
| 文本冲突，需逐块处理 | `src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`、`src-tauri/src/settings.rs`、`src-tauri/src/store.rs`、`src/lib/api.ts`、`src/main.tsx`、`src/store.ts`、`src/ui.tsx`、`src/views/Settings.tsx` |
| 当前与祖先一致，可直接接收官方改动 | `src-tauri/src/net.rs`、`src-tauri/src/text.rs`、`src/views/Digest.tsx`、`src/views/Reader.tsx` |
| 官方新增且本地尚不存在 | `src-tauri/src/rules.rs`、`src-tauri/src/thread.rs`、`src/views/Rules.tsx` |
| 文本上可自动合并，仍需检查行为 | `package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/src/ai.rs`、`src-tauri/src/assist.rs`、`src-tauri/src/mailbox.rs`、`src-tauri/tauri.conf.json`、`src/styles.css`、`src/views/Compose.tsx`、`src/views/Guide.tsx`、`src/views/MailList.tsx` |

尤其检查 `mailbox.rs`：它既接入会话归并、自动规则和验证码通知，又已有 fork 的垃圾箱移动、失败回滚与计数修复。文本自动合并通过不能证明这些功能协作正确。

## 6. 建议集成流程

建议在 fork 功能分支上做一次有记录的人工 merge，保留上游祖先关系，并在合并提交说明中明确排除了官方 MCP。这比把多个共享文件整份覆盖更便于后续同步。

以下为分析阶段提出的实施步骤，供理解处理顺序；实际执行使用 `codex/upstream-0.3.2` 分支并一次合入 `fef87ac`，详见文首。不要在已合并的分支重复执行这些历史命令。

```powershell
git status --short
git switch -c codex/upstream-0.3.2-no-official-mcp
git merge --no-ff --no-commit 935b1d8d07747433f401c7fbd4e875a9f2f0041a
```

出现冲突是预期行为。先保留 fork 的 MCP 主文件：

```powershell
git restore --source=HEAD --staged --worktree -- src-tauri/src/mcp.rs
```

然后按以下逻辑解决其余文件：

1. 接入 `thread.rs`、`rules.rs`、`Rules.tsx` 及对应命令、API、状态、页面。
2. 在共享文件中排除第 3 节列出的官方 MCP 接线；保留 fork 原有权限和 scope 逻辑。
3. 按第 4 节追加数据库迁移，修正版本号和相关说明。
4. 接入验证码、自定义要求、代理和界面细节。
5. 保留 fork 已有的正文/签名/引用统一渲染、`DraftContent`、签名折叠、邮件移动修复、更新弹窗、Windows 安装钩子和 MCP sidecar。
6. 只更新包版本等必要项，不用官方整份 manifest 覆盖 fork 的依赖和 workspace；核对 `package-lock.json` 根包版本，并用正常依赖工具保持锁文件一致。
7. 运行第 7 节验证，逐项暂存已解决文件，再用 `git diff --cached` 审核并创建合并提交。

可用 `git diff --name-only --diff-filter=U` 查看尚未解决的冲突。`git diff --check` 检查空白和意外残留标记，但不能代替审阅所有官方 MCP 接线。需要取消这次尚未提交的合并时，在干净工作区起步的前提下使用 `git merge --abort`。

官方 `fef87ac` 仅修改 README，可在功能集成后按段吸收非 MCP 的说明；不要把 fork 现有 MCP 文档整体替换成官方只读实现的说明。

## 7. 合并完成标准

基础检查：

```powershell
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --workspace
git diff --check
```

还需验证这些具体行为：

- 新建库、旧官方 0.3.1 的 v2 库、fork 的 v3 草稿库都能到目标版本，保留草稿、批注、待办与账号 MCP 权限；二次启动不会重复迁移。若支持官方 0.3.2 数据库，补充官方 v3/v4 来源用例。
- 会话按账号隔离；乱序引用能归并；Gmail 多文件夹副本去重；搜索仍逐封；删除、回复和快捷键作用于当前单封。
- 规则只处理新到收件箱邮件；首次同步、服务器重建、手动/MCP 移动不会反复触发；规则队列与已有移动失败回滚协作正常。
- 验证码能从主题/正文提取，通知不会重复，复制需要用户点击；无正文到达时超时仍能通知。
- 代理三种模式、地址校验、自定义代理不可用时的报错与重连生效行为正常。
- 自定义 AI 要求能保存、限制长度并应用到对应入口；已有 MCP AI 工具仍尊重原权限。
- 普通发送、MCP 草稿/发送、签名、引用和预览继续使用 fork 的统一渲染；仅编辑草稿正文不清空主题和收件人。
- MCP 关闭时不监听；全局与逐账号授权继续有效；发送/永久删除仍按既有流程确认；不出现官方 `--mcp` 第二入口。
- Windows 构建仍带原来的 MCP 代理和安装钩子，更新弹窗保持可用。

当前 updater 配置仍指向官方 `https://mail.songxxai.com/kojoomail/update/latest.json`。如果希望长期保留 fork 改动，发布前应单独决定 fork 的更新来源和签名策略；继续安装官方更新可能用官方程序替换本地定制功能。

分析阶段的文本预演本身不构成验证。实际完成的构建和测试、尚未执行的真实环境验证均已列于文首。

## 8. 依据

- 从官方 remote 实际 fetch 得到的 `31361a6..935b1d8` 提交历史、完整 diff 和 `935b1d8..fef87ac` README diff。
- 本地 `ci/mcp-p2` 的提交历史、暂存与工作区内容；远程 fork 分支通过 `git ls-remote` 核对。
- 官方 `rules.rs`、`thread.rs`、`mcp.rs` 及共享设置、数据库、后端命令和前端调用的源码。
- 临时目录中的逐文件三方文本预演。
