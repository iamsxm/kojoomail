# 参与 Kojoo Mail

谢谢你愿意帮忙。

## 提问题

- Bug：写清系统版本、Kojoo Mail 版本（设置 → 关于）、邮箱服务商、怎么复现。可以附上 设置 → 关于 →「导出诊断信息」生成的文件（邮箱地址已打码，不含密码和邮件内容），贴出来之前自己再看一眼
- 功能建议：先开 Issue 聊，再动手写代码，免得白做

## 提代码

1. 一个 Pull Request 只做一件事
2. 提交前跑一下：
   ```bash
   node node_modules/typescript/bin/tsc --noEmit
   cd src-tauri && cargo test --lib
   ```
3. 界面文字用中文，和现有说法保持一致
4. 别加定时轮询、别加常驻后台任务：省资源是这个项目的硬要求（每个账号一条连接、空闲不占 CPU）

## 动 MCP 相关代码时

MCP 那条通道让外部 agent 读写邮件，几条规矩不能破（安全模型见 [SECURITY.md](SECURITY.md)）：

- 不加定时器，包括 SSE 心跳。`mcp.rs` 里显式关掉了 rmcp 默认的 15 秒 keep-alive，别打开
- 新工具必须走 `mcp_scope` 判权限，且每次调用现算，不缓存；账号白名单要一路带进 SQL
- 越权和「不存在」返回同一句话，不要新增能区分两者的错误文案
- 读类工具不写库、不派 AI 任务、不返回附件字节
- 发信只有「只存草稿」和「每次桌面确认」两档，不要加第三档；确认框文案不能由 agent 控制，收件人要完整列出
- 关权限时工具要从清单里消失（`disable_route`），不是调用时才报错
- 协议握手、`list_changed` 推送和桌面确认框没法用 `cargo test` 覆盖：`Ctx` 绑的是真实 Tauri runtime，mock 出来的 `AppHandle` 类型对不上。动这几块要拿真 MCP 客户端连一遍，别只跑单测就以为过了

## 授权约定（提交前请读）

本项目以 [AGPL-3.0](LICENSE) 开源，同时保留以其他许可证（比如商业授权）发布的权利，收入用来维持项目。

提交 Pull Request 即表示你同意：

1. 你的贡献是你自己写的，或者你有权按下面的条款提交；
2. 你的贡献按 AGPL-3.0 授权给所有人；
3. 你同时授予项目维护者一项永久、全球、免费、不可撤销的许可，可以把你的贡献用在本项目以其他许可证（包括商业许可证）发布的版本中。

你保留自己贡献的版权。

---

## Contributing (English)

Bug reports and pull requests are welcome. Keep each PR focused, run `tsc --noEmit` and `cargo test --lib` before submitting, and avoid adding polling or background timers.

When touching MCP code: no timers of any kind (including the SSE keep-alive that `mcp.rs` deliberately disables), every new tool checks permissions through `mcp_scope` on each call, unauthorized and missing resources return the identical error, read tools never write or spend AI quota, sending keeps exactly two modes (draft only / confirm each send), and revoking a permission removes the tool from the list rather than failing at call time. The protocol handshake, `list_changed` notifications and desktop confirmation dialogs cannot be covered by `cargo test` — `Ctx` holds a real Tauri runtime handle, so a mocked app handle does not type-check; exercise those with a real MCP client. See [SECURITY.md](SECURITY.md).

By submitting a pull request you agree that (1) the contribution is your own work or you have the right to submit it, (2) it is licensed to everyone under AGPL-3.0, and (3) you grant the project maintainer a perpetual, worldwide, royalty-free, irrevocable license to include it in versions of this project released under other licenses, including commercial ones. You keep the copyright to your contribution.
