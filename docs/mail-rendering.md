# 统一发信渲染

普通发送、MCP 新建/回复/转发、写信预览和签名预览，都使用 `src-tauri/src/html.rs` 中的 `render`。前端没有第二个 Markdown 渲染器；读取来信的独立 iframe 仍由 Reader 做额外隔离。

## 开源基础组件

| 组件 | 职责 | 项目 |
| --- | --- | --- |
| pulldown-cmark | CommonMark，以及表格、删除线、任务列表；保留邮件中的普通换行 | https://github.com/pulldown-cmark/pulldown-cmark |
| ammonia | 基于 HTML 解析器清理标签、属性、链接与内联 CSS | https://github.com/rust-ammonia/ammonia |
| html2text | 从清理后的 HTML 生成可读的 MIME 纯文本，保留列表、链接目标和代码 | https://github.com/jugglerchris/rust-html2text |
| linkify | 识别裸网址和邮箱，避免对代码及已有链接重复建链接 | https://github.com/robinst/linkify |

项目的邮件样式白名单进一步限制 CSS 值、声明数量、图片大小和链接协议。正文、签名和引用共用这一份策略；脚本、表单、全局样式、事件、危险协议、定位和 CSS 外部资源不会进入预览或发送结果。

## 内容与出口

`MailContent` 分别保存原始 `body`、`signature` 和 `quote`。正文与签名各自解析 Markdown/HTML，防止未闭合的代码块或 HTML 影响下一段。引用包含原邮件 HTML、备用纯文本和单独的邮件头；有 HTML 时只清洗，不再次按 Markdown 解析或转义。纯文本引用保留字面字符与缩进。

`render` 返回：

- `preview`：清理并拼接后的 HTML 片段。
- `html`：同一片段加上发信用字体外层。
- `text`：可独立阅读的纯文本，签名使用 RFC 3676 的 `-- ` 分隔行。

前端 `render_mail` 命令仅转调 `render`；`send_mail_inner` 在生成 SMTP/Graph 共用的 MIME 前重新调用同一函数。旧客户端传入的 `html` 字段被忽略，不能绕过统一渲染。预览请求按内容失效，旧请求不会覆盖新内容，失败会显示错误。

MCP 新建邮件和普通写信一样自动附加账号签名。`mail_reply`/`mail_forward` 生成结构化引用，CID 图片沿用普通读信的本地内嵌图片转换；引用 HTML 随后经过共享清理。小体积 PNG/JPEG/GIF/WebP 的 data URI 保留，SVG、未知 MIME 和超过 256 KiB 的 data URI 移除。

## 草稿兼容

数据库版本 3 增加可空的 `drafts.content_json`，分别保存三个内容部分。原 `body` 列保留为可读纯文本，供列表和 MCP 查询。升级前沿用现有机制备份 SQLite；旧草稿保留原文，按已知引用头和签名分隔恢复，保存后转为结构化格式。

重开草稿或撤销发送直接恢复结构化正文、引用，并使用当前发件账号的签名。MCP 通过草稿 id 编辑或发送时也恢复原结构；把列表返回的完整纯文本原样传回不会再次追加其中的签名和引用。

旧的无分隔符签名如果已经修改，无法可靠判断其边界，不会任意删除疑似正文。HTML 实体只在正确的内容类型下处理，不递归解码代码示例中的 `&nbsp;` 或字面的 HTML。

## 验证

- `npm test`：预览请求顺序、账号/内容变化、失败恢复、卸载，以及既有 MCP 配置测试。
- `npm run build`：TypeScript 和前端生产构建。
- `cd src-tauri; cargo test --lib --offline`：共享 Markdown/HTML/CSS/协议策略、签名与引用隔离、实体与代码、旧库升级，以及新建/回复/全部回复/转发的 UI、MCP、预览、草稿往返和最终 MIME 一致性。

测试使用临时数据库和本地 MIME 构造，不向外发送邮件。
