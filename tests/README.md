# 测试

| 文件 | 用途 |
|---|---|
| `mail-preview.test.mjs` | 共享后端渲染的异步预览：请求顺序、内容变化、失败与卸载；`npm test` 执行 |
| `../src-tauri/src/html.rs`、`mcp_tools.rs` 的渲染测试 | Markdown/HTML/CSS 清理、签名/引用隔离，以及普通发送/MCP/预览/草稿/MIME 一致性；设计说明见 `../docs/mail-rendering.md` |
| `mailbox.rs`、`store.rs`、`commands.rs` 的移动测试 | 使用内存 IMAP 通道验证 MOVE/COPY 路径转义和 CRLF；校验移动期间计数、重复请求和失败回滚，不操作真实邮箱 |
| `mcp_tools.rs` 的草稿编辑测试 | 只改正文时保留账号、收件人、主题、附件、引用与回复模式；显式空数组/空字符串允许清空 |
| `../src-tauri` 里 `cargo test --lib` | 单元测试：HTML 转文本、文件夹名解码、注入过滤、邮件解析、JSON 容错、一键登录流程（假登录服务器）、代理隧道 |
| `e2e/flow.js` | 端到端：启动真实程序 → 添加账号 → 读信 → AI → 发信给自己 → 归档撤销 → 右键 → 待办 → 深色 → 内存 → 删账号 |
| `e2e/mem.ps1` | 统计程序和 WebView2 子进程的内存（按 `--webview-exe-name=kojoomail.exe` 认进程；必须纯 ASCII，PowerShell 5 按 ANSI 读） |
| `e2e/idle.sh` | 空闲内存/CPU：`KJM_KEEP=1` 跑完 flow.js 保留数据目录，再 `bash idle.sh <exe> <数据目录>` 分前台和托盘两种量 |

端到端流程：
1. 准备一个能用密码登录 IMAP/SMTP 的测试邮箱（`flow.js` 默认在欢迎页选「kojoo.co」，换别的服务商改脚本里那一行），最好先往收件箱放几封样例邮件
2. 编调试版：`node node_modules/@tauri-apps/cli/tauri.js build --debug --no-bundle`
3. `cd tests/e2e && npm ci && KJM_AI_KEY=<可选> node flow.js <调试版 kojoomail.exe> <截图输出目录> <测试邮箱> <密码>`

测试实例用临时数据目录（`KOJOOMAIL_DATA`）和单独的凭据命名空间，不碰本机正式数据。
