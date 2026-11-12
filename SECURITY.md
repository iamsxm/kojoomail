# 安全问题反馈

Kojoo Mail 经手你的邮件、邮箱密码和 AI 密钥，安全问题我们会优先处理。

**发现安全漏洞，请不要公开提 Issue**，发邮件到 eko@kojoo.co，标题写「Kojoo Mail 安全问题」。请写清：

- 影响的版本和系统（Windows / macOS）
- 问题是什么、怎么复现
- 可能造成的影响，比如能读到别人的邮件、拿到密码，或者能让软件执行别的代码

我们会在 3 个工作日内回复，修好后发新版本，并在更新说明里致谢（你不想署名的话可以不写）。修复发布之前，请不要公开细节。

只修最新版本：发现问题后请先更新到最新版确认还在。

## MCP 服务的安全模型

设置 → MCP 服务可以让本机的 AI agent 通过标准 MCP 协议访问邮件。这条通道**默认关闭**，关着的时候不监听端口、不跑任何后台任务。开启后：

- **只监听 127.0.0.1**，局域网访问不到。每个请求都要带 Bearer 令牌（定长比较），并校验 Host、拒绝一切带 `Origin` 头的请求，挡掉 DNS rebinding 和恶意网页。
- **读、写、发三条轴各自独立**，每条都是「全局开关 + 每账号开关」两层与运算，每次调用现算。读在开启 MCP 后默认允许，写和发默认关闭，且必须逐账号再开一次。
- **发信只有两档**：只存草稿（默认）、每次弹桌面确认。**没有无人值守直发**。确认档下每封信发出前都会弹框，完整列出发件账号、全部收件人、主题、附件和正文开头；点拒绝、关掉窗口、120 秒无响应都算拒绝。永久删除在任何设置下都要确认。
- **越权和「邮件不存在」返回逐字相同的错误**，不泄露某个账号里有没有这封信。批量操作要么全部通过要么整批不动。
- 附件只能存到「下载」文件夹，文件名清洗过，重名加序号，绝不覆盖。
- 来信内容一律当不可信数据：确认框里的文字先过滤指令句式和控制字符再显示，并标明来自 AI、未经核实。
- 令牌存在数据目录的 `mcp-token` 里，和邮件数据库同目录、同访问权限。设置页可以重新生成，已配置的 agent 会全部失效。

应用内的 AI 助手不受这些开关影响，它**始终只能起草，不能发送**。

---

# Security policy

Kojoo Mail handles your email, mailbox passwords and AI keys, so security issues get priority.

**Please don't report security vulnerabilities in public issues.** Email eko@kojoo.co with the subject "Kojoo Mail security issue" and include:

- the affected version and system (Windows / macOS)
- what the problem is and how to reproduce it
- the possible impact, such as reading other people's mail, obtaining passwords, or making the app run other code

We'll reply within 3 business days, release a fix, and credit you in the release notes (or not, if you prefer). Please don't disclose details publicly until the fix is out.

Only the latest version is supported: please check the issue still exists on the latest release.

## MCP server security model

Settings → MCP server lets AI agents on this computer reach your mail over the standard MCP protocol. The channel is **off by default**; while off it holds no port and runs no background task. Once enabled:

- It **listens on 127.0.0.1 only** and is not reachable from the network. Every request must carry a Bearer token (compared in constant time), the Host header is validated, and any request carrying an `Origin` header is rejected — blocking DNS rebinding and malicious web pages.
- **Read, write and send are three independent axes**, each requiring both a global switch and a per-account switch, recomputed on every call. Reading is allowed once MCP is on; writing and sending are off by default and must be enabled per account.
- **Sending has exactly two modes**: draft only (the default) and confirm each send. **There is no unattended send.** In confirm mode a desktop dialog lists the sending account, every recipient, the subject, attachments and the start of the body before anything leaves; denying, closing the window or 120 seconds of silence all count as refusal. Permanent deletion always asks, whatever the settings.
- **An unauthorized request and a missing message return the identical error**, so nothing reveals whether an account holds a given email. Batch operations either pass entirely or change nothing.
- Attachments can only be written to your Downloads folder, with sanitized names, a numbered suffix on clashes, and never an overwrite.
- Incoming mail is always treated as untrusted: text shown in the confirmation dialog is stripped of instruction patterns and control characters, and labelled as coming from an AI and unverified.
- The token lives in `mcp-token` in the data directory, next to the mail database and under the same access controls. You can regenerate it in settings, which disconnects every configured agent.

The in-app AI assistant is unaffected by these switches: it **only ever drafts, never sends**.
