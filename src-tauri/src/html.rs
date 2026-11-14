//! 发信与预览的唯一渲染器：pulldown-cmark → ammonia → HTML / text/plain。
//! 正文、签名分别解析，引用已有 HTML 只清洗，绝不再当 Markdown 或已转义的文本拼接。

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use pulldown_cmark::{html, Alignment, Event, LinkType, Options, Parser, Tag, TagEnd};
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MailContent {
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub quote: Option<MailQuote>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MailQuote {
    #[serde(default)]
    pub header: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub html: String,
    #[serde(default)]
    pub reply: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct RenderedMail {
    pub html: String,
    pub text: String,
    pub preview: String,
}

const SIG_RULE: &str = "<div style=\"border-top:1px solid #d8d8d8;margin:18px 0 12px\"></div>";
const TAGS: &str = "a abbr b bdi bdo big blockquote br caption center cite code col colgroup dd del div dl dt em figcaption figure font h1 h2 h3 h4 h5 h6 hr i img ins kbd li mark ol p pre q s samp small span strike strong sub sup table tbody td tfoot th thead tr tt u ul var wbr";
const ATTRS: &str = "align bgcolor color dir height lang style title valign width";
const CSS_PROPS: &str = "color font font-family font-size font-style font-variant font-weight letter-spacing line-height text-align text-decoration text-decoration-color text-decoration-line text-decoration-style text-indent text-transform text-shadow vertical-align white-space word-break word-spacing overflow-wrap word-wrap direction unicode-bidi width height min-width max-width min-height max-height box-sizing aspect-ratio object-fit background background-color border border-width border-style border-color border-radius border-collapse border-spacing box-shadow outline outline-color outline-style outline-width table-layout caption-side empty-cells list-style list-style-type list-style-position float clear display overflow overflow-x overflow-y margin padding";

fn css_props() -> &'static HashSet<String> {
    static PROPS: OnceLock<HashSet<String>> = OnceLock::new();
    PROPS.get_or_init(|| {
        let mut p: HashSet<String> = CSS_PROPS.split_whitespace().map(str::to_owned).collect();
        for side in ["top", "right", "bottom", "left"] {
            for prefix in ["margin", "padding", "border"] {
                p.insert(format!("{prefix}-{side}"));
            }
            for suffix in ["width", "style", "color"] {
                p.insert(format!("border-{side}-{suffix}"));
            }
        }
        for corner in ["top-left", "top-right", "bottom-right", "bottom-left"] {
            p.insert(format!("border-{corner}-radius"));
        }
        p
    })
}

fn clean_css(css: &str) -> String {
    static BAD: OnceLock<Regex> = OnceLock::new();
    let bad = BAD.get_or_init(|| Regex::new(r"(?i)url\s*\(|var\s*\(|attr\s*\(|env\s*\(|expression\s*\(|javascript:|vbscript:|behavior|-moz-binding|@import|\\|/\*|\*/|[\x00-\x1f\x7f\u{fffd}]|\d+(?:\.\d+)?(?:vw|vh|vmin|vmax)\b").unwrap());
    let mut declarations = Vec::new();
    let mut len = 0;
    for decl in css.split(';') {
        let Some((prop, value)) = decl.split_once(':') else { continue };
        let prop = prop.trim().to_ascii_lowercase();
        let value = value.trim();
        if !css_props().contains(&prop) || value.is_empty() || value.len() > 512 || bad.is_match(value) {
            continue;
        }
        let normalized = value.to_ascii_lowercase();
        let normalized = normalized.trim_end_matches("!important").trim();
        let allowed: &[&str] = match prop.as_str() {
            "display" => &["block", "inline", "inline-block", "flow-root", "table", "table-row", "table-cell", "table-caption", "list-item"],
            "float" => &["left", "right", "none"],
            "clear" => &["left", "right", "both", "none"],
            "overflow" | "overflow-x" | "overflow-y" => &["visible", "hidden", "auto", "scroll"],
            _ => &[],
        };
        if !allowed.is_empty() && !allowed.contains(&normalized) {
            continue;
        }
        len += prop.len() + value.len();
        if len > 8192 || declarations.len() == 64 {
            break;
        }
        declarations.push(format!("{prop}:{value}"));
    }
    declarations.join(";")
}

fn safe_uri(tag: &str, attr: &str, value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("data:") {
        return tag == "img"
            && attr == "src"
            && value.len() <= 256 * 1024
            && ["png", "jpeg", "jpg", "gif", "webp"].iter().any(|mime| lower.starts_with(&format!("data:image/{mime};base64,")));
    }
    ["http:", "https:", "mailto:", "tel:"].iter().any(|scheme| lower.starts_with(scheme))
}

pub fn sanitize(input: &str) -> String {
    let mut tag_attrs: HashMap<&str, HashSet<&str>> = HashMap::new();
    for (attr, tags) in [
        ("alt", "img"),
        ("border", "table img"),
        ("cellpadding", "table"),
        ("cellspacing", "table"),
        ("colspan", "td th"),
        ("face", "font"),
        ("href", "a"),
        ("reversed", "ol"),
        ("rowspan", "td th"),
        ("size", "font hr"),
        ("src", "img"),
        ("start", "ol"),
        ("type", "ol ul li"),
        ("value", "li"),
    ] {
        for tag in tags.split_whitespace() {
            tag_attrs.entry(tag).or_default().insert(attr);
        }
    }
    ammonia::Builder::default()
        .tags(TAGS.split_whitespace().collect())
        .tag_attributes(tag_attrs)
        .generic_attributes(ATTRS.split_whitespace().collect())
        .clean_content_tags(["script", "style", "iframe", "object", "svg", "math", "head", "title", "template", "noscript"].into())
        .url_schemes(["http", "https", "mailto", "tel", "data"].into())
        .url_relative(ammonia::UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer"))
        .set_tag_attribute_value("a", "target", "_blank")
        .attribute_filter(|tag, attr, value| {
            if attr == "style" {
                let css = clean_css(value);
                return (!css.is_empty()).then_some(Cow::Owned(css));
            }
            if matches!(attr, "href" | "src") && !safe_uri(tag, attr, value) {
                return None;
            }
            Some(Cow::Borrowed(value))
        })
        // ammonia 的 CSS 解析器再校验一次语法；上面的过滤同时限制值、长度和声明数量。
        .filter_style_properties(css_props().iter().map(String::as_str).collect())
        .clean(input)
        .to_string()
        .trim()
        .to_string()
}

pub fn markdown(input: &str) -> String {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut columns = Vec::new();
    let mut column = 0;
    let mut header = false;
    let parser = Parser::new_ext(input, options).map(|event| match event {
        // 邮件客户端没有应用的 CSS。仅给 Markdown 生成的表格加内联样式，
        // 不改变用户 HTML 签名、布局表格或原邮件引用的边框。
        Event::Start(Tag::Table(alignments)) => {
            columns = alignments;
            Event::Html("<table border=\"1\" cellpadding=\"8\" cellspacing=\"0\" style=\"border-collapse:collapse;border:1px solid #b8c2cc;width:100%\">".into())
        }
        Event::Start(Tag::TableHead) => {
            header = true;
            column = 0;
            Event::Html("<thead><tr>".into())
        }
        Event::End(TagEnd::TableHead) => {
            header = false;
            Event::Html("</tr></thead>".into())
        }
        Event::Start(Tag::TableRow) => {
            column = 0;
            Event::Start(Tag::TableRow)
        }
        Event::Start(Tag::TableCell) => {
            let align = match columns.get(column) {
                Some(Alignment::Center) => "center",
                Some(Alignment::Right) => "right",
                _ => "left",
            };
            column += 1;
            let tag = if header { "th" } else { "td" };
            let background = if header { ";background-color:#f2f4f7;color:#1f2a37" } else { "" };
            Event::Html(format!("<{tag} style=\"border:1px solid #b8c2cc;padding:8px 10px;vertical-align:top;text-align:{align}{background}\">").into())
        }
        Event::End(TagEnd::TableCell) => Event::Html(if header { "</th>" } else { "</td>" }.into()),
        Event::SoftBreak => Event::HardBreak,
        Event::TaskListMarker(checked) => Event::Text(if checked { "☑ " } else { "☐ " }.into()),
        other => other,
    });
    // 保留原渲染器的裸网址/邮箱自动链接；已有链接、图片说明和代码中不再次建链接。
    let mut finder = linkify::LinkFinder::new();
    finder.url_must_have_scheme(false);
    let mut suppressed = 0u32;
    let mut raw_suppressed = 0u32;
    let parser = parser.flat_map(|event| {
        match &event {
            Event::Start(Tag::Link { .. } | Tag::Image { .. } | Tag::CodeBlock(_)) => suppressed += 1,
            Event::End(TagEnd::Link | TagEnd::Image | TagEnd::CodeBlock) => suppressed = suppressed.saturating_sub(1),
            Event::InlineHtml(raw) => {
                let raw = raw.trim();
                if let Some(tag) = raw.strip_prefix('<') {
                    let closing = tag.starts_with('/');
                    let name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '>' || c == '/').next().unwrap_or("");
                    if ["a", "pre", "code"].iter().any(|n| name.eq_ignore_ascii_case(n)) {
                        if closing {
                            raw_suppressed = raw_suppressed.saturating_sub(1);
                        } else if !raw.ends_with("/>") {
                            raw_suppressed += 1;
                        }
                    }
                }
            }
            _ => (),
        }
        if suppressed == 0 && raw_suppressed == 0 {
            if let Event::Text(text) = &event {
                let mut events = Vec::new();
                let mut end = 0;
                for link in finder.links(text) {
                    // 中文标点常直接紧跟网址，通用 linkify 会把其后的一整句也当成路径。
                    let address = link
                        .as_str()
                        .split(['，', '。', '；', '！', '？', '、', '：', '（', '）', '【', '】', '《', '》', '“', '”', '‘', '’'])
                        .next()
                        .unwrap_or("");
                    let lower = address.to_ascii_lowercase();
                    let destination = if *link.kind() == linkify::LinkKind::Email {
                        format!("mailto:{address}")
                    } else if lower.starts_with("http:") || lower.starts_with("https:") {
                        address.to_string()
                    } else if lower.starts_with("www.") {
                        format!("https://{address}")
                    } else {
                        continue;
                    };
                    events.push(Event::Text(text[end..link.start()].to_string().into()));
                    events.push(Event::Start(Tag::Link { link_type: LinkType::Autolink, dest_url: destination.into(), title: "".into(), id: "".into() }));
                    events.push(Event::Text(address.to_string().into()));
                    events.push(Event::End(TagEnd::Link));
                    end = link.start() + address.len();
                }
                if end > 0 {
                    events.push(Event::Text(text[end..].to_string().into()));
                    return events;
                }
            }
        }
        vec![event]
    });
    let mut html = String::new();
    html::push_html(&mut html, parser);
    sanitize(&html)
}

pub fn escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

fn text_html(input: &str) -> String {
    escape(input).replace("\r\n", "\n").replace('\r', "\n").replace('\n', "<br>")
}

fn plain(input: &str) -> String {
    html2text::config::plain_no_decorate()
        .link_footnotes(true)
        .no_link_wrapping()
        .allow_width_overflow()
        .string_from_read(input.as_bytes(), 1000)
        .unwrap_or_else(|_| crate::text::html_to_text(input))
        .trim()
        .to_string()
}

pub fn render(content: &MailContent) -> RenderedMail {
    let body = markdown(&content.body);
    let signature = markdown(&content.signature);
    let mut preview = body.clone();
    let mut text = plain(&body);
    if !signature.is_empty() {
        if !body.is_empty() {
            preview.push_str(SIG_RULE);
        }
        preview.push_str(&signature);
        let sig_text = plain(&signature);
        if !text.is_empty() && !sig_text.is_empty() {
            text.push_str("\n\n-- \n");
        }
        text.push_str(&sig_text);
    }
    if let Some(q) = &content.quote {
        let clean = sanitize(&q.html);
        let (quote_html, quote_text) = if clean.is_empty() {
            (format!("<div style=\"white-space:pre-wrap\">{}</div>", escape(&q.text)), q.text.clone())
        } else {
            (clean.clone(), plain(&clean))
        };
        preview.push_str("<div style=\"margin-top:20px\">");
        if !q.header.is_empty() {
            preview.push_str(&format!("<p>{}</p>", text_html(&q.header)));
        }
        preview.push_str(&format!("<blockquote style=\"margin:0;padding-left:12px;border-left:2px solid #d8d8d8\">{quote_html}</blockquote></div>"));
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        if !q.header.is_empty() {
            text.push_str(&q.header);
            text.push('\n');
        }
        if q.reply {
            text.push_str(&quote_text.lines().map(|l| format!("> {l}")).collect::<Vec<_>>().join("\n"));
        } else {
            text.push_str(&quote_text);
        }
    }
    let html = format!(
        "<div style=\"font-family:-apple-system,'Segoe UI','Microsoft YaHei',sans-serif;font-size:14px;line-height:1.6;color:#1F2A37\">{preview}</div>"
    );
    RenderedMail { html, text, preview }
}

/// 旧草稿只存了一段正文：仅按客户端写入的引用头与当前签名恢复，避免重复追加签名。
/// 未知内容原样保留；新草稿使用结构化数据，不需要再猜分隔位置。
pub fn restore_legacy(body: &str, signature: &str) -> MailContent {
    let normalized = body.replace("\r\n", "\n").replace('\r', "\n");
    let body = normalized.as_str();
    static QUOTE: OnceLock<Regex> = OnceLock::new();
    let pattern =
        QUOTE.get_or_init(|| Regex::new(r"\n\n(?:---------- (?:Forwarded message|转发的邮件) ----------|On [^\n]+ wrote:|在 [^\n]+ 写道：)\r?\n").unwrap());
    let (authored, quote) = if let Some(found) = pattern.find(body) {
        (&body[..found.start()], Some(MailQuote { text: body[found.start()..].trim_start().to_string(), ..Default::default() }))
    } else {
        (body, None)
    };
    let mut authored = authored.to_string();
    if let Some((text, _old_signature)) = authored.rsplit_once("\n\n-- \n") {
        // RFC 3676 分隔符也能识别签名已修改/删除的旧草稿。
        authored = text.to_string();
    } else if !signature.is_empty() {
        for suffix in [format!("\n\n-- \n{signature}"), format!("\n\n{signature}")] {
            if let Some(rest) = authored.strip_suffix(&suffix) {
                authored = rest.to_string();
                break;
            }
        }
        if authored == signature {
            authored.clear();
        }
    }
    MailContent { body: authored, signature: signature.into(), quote }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_and_inline_html_share_formatting() {
        let html = markdown("# 标题\n\n**加粗** *斜体* ~~删除~~\n下一行\n\n- 项目\n\n<span style=\"color:#3F4AAB\">**蓝色加粗**</span>");
        for expected in
            ["<h1>标题</h1>", "<strong>加粗</strong>", "<em>斜体</em>", "<del>删除</del>", "<br", "<li>项目</li>", "color:#3F4AAB", "<strong>蓝色加粗</strong>"]
        {
            assert!(html.contains(expected), "missing {expected}: {html}");
        }
    }

    #[test]
    fn gfm_tables_and_tasks_remain_readable_without_form_controls() {
        let html = markdown("| A | B |\n|---|---|\n| 一 | 二 |\n\n- [x] 完成\n- [ ] 待办");
        for expected in ["<table border=\"1\"", "border-collapse:collapse", "border:1px solid #b8c2cc", ">A</th>", ">一</td>", "☑ ", "☐ "] {
            assert!(html.contains(expected), "{html}");
        }
        assert!(!html.contains("<input"), "{html}");
    }

    #[test]
    fn sent_markdown_tables_have_inline_borders_and_keep_alignment() {
        let out = render(&MailContent { body: "您好！\n\n| 项目 | 本周（11-09 \\~ 11-13） | 下周 | 里程碑 | 风险 |\n|:---|:---:|---:|---|---|\n| 平台 | ① 完成 **优化**；② 验证 | 跟进 | 已上线 | 无 |\n\n以上，请审阅。".into(), ..Default::default() });
        assert_eq!(out.html.matches("border:1px solid #b8c2cc").count(), 11);
        for expected in ["text-align:left", "text-align:center", "text-align:right", "<strong>优化</strong>", "11-09 ~ 11-13", "以上，请审阅。"] {
            assert!(out.html.contains(expected), "{out:?}");
        }
        assert!(out.html.contains(&out.preview));
        assert!(out.text.contains("优化") && out.text.contains("平台"));
        let legacy = markdown("<table border=\"0\"><tr><td>签名布局</td></tr></table>");
        assert!(!legacy.contains("#b8c2cc"));
    }

    #[test]
    fn rich_table_layout_survives_send_sanitization() {
        let out = render(&MailContent {
            body: "<table data-kojoomail-table=\"1\" border=\"1\" cellspacing=\"0\" style=\"border-collapse:collapse;border:1px solid #b8c2cc\"><colgroup><col style=\"width:220px\"><col style=\"min-width:40px\"></colgroup><tbody><tr><th colspan=\"2\" style=\"border:1px solid #b8c2cc;padding:8px 10px\"><p>标题</p></th></tr><tr><td rowspan=\"2\" style=\"border:1px solid #b8c2cc\"><p>第一段</p><p>第二段</p></td><td>正文</td></tr><tr><td>末行</td></tr></tbody></table>".into(),
            ..Default::default()
        });
        for expected in ["border-collapse:collapse", "width:220px", "min-width:40px", "colspan=\"2\"", "rowspan=\"2\"", "<p>第一段</p><p>第二段</p>"] {
            assert!(out.html.contains(expected), "{out:?}");
        }
        assert!(!out.html.contains("data-kojoomail-table"));
        assert!(out.text.contains("第一段") && out.text.contains("末行"));
    }

    #[test]
    fn visual_fonts_and_colors_survive_send_without_editor_metadata() {
        let out = render(&MailContent {
            body: "正常正文\n\n<span data-kojoomail-style=\"1\" style=\"font-family: Microsoft YaHei, PingFang SC, sans-serif; font-size: 24px; color: rgb(37, 99, 235); background-color: rgb(254, 240, 138);\">**重点** [链接](https://example.com)</span>\n\n| 项目 | 状态 |\n|---|---|\n| 平台 | <span data-kojoomail-style=\"1\" style=\"color: rgb(220, 38, 38);\">待处理</span> |".into(),
            ..Default::default()
        });
        for expected in ["font-family:Microsoft YaHei", "PingFang SC", "sans-serif", "font-size:24px", "color:rgb(37, 99, 235)", "background-color:rgb(254, 240, 138)", "<strong>重点</strong>", "href=\"https://example.com\"", "border:1px solid #b8c2cc", "待处理"] {
            assert!(out.html.contains(expected), "{expected}: {out:?}");
        }
        assert!(!out.html.contains("data-kojoomail-style"));
        assert!(out.html.contains(&out.preview));
        assert!(out.text.contains("正常正文") && out.text.contains("重点") && out.text.contains("待处理"));
    }

    #[test]
    fn bare_links_and_emails_are_linked_except_inside_code_and_existing_links() {
        let html = markdown("访问 https://example.com/path?q=1&x=2，联系 alice@example.com 或 www.example.com。\n\n`https://code.example`\n\n[https://label.example](https://target.example)\n\n<a href=\"https://original.example\">https://inside.example</a>\n\n<code>https://raw-code.example</code>\n\n```\nhttps://block-code.example\n```");
        for expected in [
            "href=\"https://example.com/path?q=1&amp;x=2\"",
            "href=\"mailto:alice@example.com\"",
            "href=\"https://www.example.com\"",
            "href=\"https://target.example\"",
            "href=\"https://original.example\"",
        ] {
            assert!(html.contains(expected), "{expected}: {html}");
        }
        for address in ["code.example", "label.example", "inside.example", "raw-code.example", "block-code.example"] {
            assert!(!html.contains(&format!("href=\"https://{address}")), "{html}");
        }
    }

    #[test]
    fn full_document_quotes_drop_head_contents_and_preserve_body() {
        let out = render(&MailContent { quote: Some(MailQuote { html: "<!doctype html><html><head><title>文档标题</title><style>body{display:none}</style></head><body><p style=\"color:red\">原文</p></body></html>".into(), ..Default::default() }), ..Default::default() });
        assert!(out.html.contains("<p style=\"color:red\">原文</p>"));
        assert!(!out.html.contains("文档标题") && !out.html.contains("display:none"));
    }

    #[test]
    fn safe_html_signature_keeps_colors_table_layout_and_images() {
        let html = markdown("<table border=\"0\" cellpadding=\"4\"><tr><td valign=\"top\" style=\"padding-right:10px\"><font color=\"#1B4A80\">公司</font></td><td><img src=\"https://example.com/logo.png\" width=\"120\" alt=\"公司标志\"></td></tr></table>");
        for expected in ["border=\"0\"", "cellpadding=\"4\"", "valign=\"top\"", "padding-right:10px", "color=\"#1B4A80\"", "width=\"120\"", "alt=\"公司标志\""]
        {
            assert!(html.contains(expected), "missing {expected}: {html}");
        }
    }

    #[test]
    fn scripts_forms_events_and_document_styles_cannot_escape_preview() {
        let html = markdown("<style>body{display:none}</style><script>alert(1)</script><iframe>frame</iframe><object>object</object><svg onload=\"alert(1)\"></svg><math>x</math><form action=\"https://evil.example\"><input name=\"a\"><button>go</button></form><img src=\"x\" onerror=\"alert(1)\"><div id=\"app\" class=\"btn\" data-x=\"1\" onclick=\"alert(1)\">正文</div>");
        for bad in [
            "<script",
            "<style",
            "display:none",
            "alert(",
            "<iframe",
            "<object",
            "<svg",
            "<math",
            "<form",
            "<input",
            "<button",
            "onerror",
            "onclick",
            "data-x",
            "id=",
            "class=",
        ] {
            assert!(!html.contains(bad), "{bad}: {html}");
        }
        assert!(html.contains("正文"));
    }

    #[test]
    fn css_keeps_layout_and_rejects_dangerous_values() {
        let html = markdown("<div style=\"position:fixed;top:0;left:0;z-index:9999;transform:scale(3);animation:x 1s;display:none;visibility:hidden;opacity:0;color:red;font-size:12px;background:#fff3e0;padding:4px\">x</div>");
        for bad in ["position", "top:", "left:", "z-index", "transform", "animation", "display:none", "visibility", "opacity"] {
            assert!(!html.contains(bad), "{html}");
        }
        for good in ["color:red", "font-size:12px", "background:#fff3e0", "padding:4px"] {
            assert!(html.contains(good), "{html}");
        }
        for bad in ["url(https://evil.example/p.gif)", "var(--accent)", "expression(alert(1))", "r\\65d", "re/**/d", "red\u{0}", "100vw", "100vh", "10Vmin"] {
            let css = clean_css(&format!("color:{bad};width:{bad}"));
            assert!(css.is_empty(), "{bad} -> {css}");
        }
        assert_eq!(clean_css("display:inline-block;float:left;clear:both;overflow:hidden"), "display:inline-block;float:left;clear:both;overflow:hidden");
    }

    #[test]
    fn css_declarations_and_values_are_bounded() {
        let many = (0..200).map(|i| format!("margin-top:{i}px")).collect::<Vec<_>>().join(";");
        assert_eq!(clean_css(&many).matches("margin-top").count(), 64);
        assert!(clean_css(&format!("color:{}", "r".repeat(9000))).is_empty());
    }

    #[test]
    fn links_are_absolute_allowed_and_hardened() {
        for href in ["https://example.com", "http://example.com", "mailto:a@example.com", "tel:+8613800000000"] {
            let html = markdown(&format!("<a href=\"{href}\">链接</a>"));
            assert!(html.contains(&format!("href=\"{href}\"")), "{html}");
            assert!(html.contains("target=\"_blank\"") && html.contains("rel=\"noopener noreferrer\""), "{html}");
        }
        for href in [
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            "java&#x73;cript:alert(1)",
            "data:text/html;base64,PHNjcmlwdD4=",
            "data:image/png;base64,AAAA",
            "vbscript:msgbox(1)",
            "file:///c:/windows/win.ini",
            "/relative",
            "//example.com",
            "#anchor",
        ] {
            let html = markdown(&format!("<a href=\"{href}\">链接</a>"));
            assert!(!html.contains("href="), "{href} -> {html}");
        }
        assert!(!sanitize("<div href=\"https://example.com\">x</div><td src=\"https://example.com/p.gif\">y</td>").contains("href="));
    }

    #[test]
    fn embedded_images_are_mime_and_size_limited() {
        for mime in ["png", "jpeg", "jpg", "gif", "webp"] {
            let html = markdown(&format!("<img src=\"data:image/{mime};base64,AAAA\">"));
            assert!(html.contains("src=\"data:image/"), "{html}");
        }
        for src in ["data:image/svg+xml;base64,PHN2Zz4=", "data:text/html;base64,PHNjcmlwdD4=", "cid:image1", "file:///logo.png", "/logo.png"] {
            assert!(!markdown(&format!("<img src=\"{src}\">")).contains("src="), "{src}");
        }
        assert!(!markdown(&format!("<img src=\"data:image/png;base64,{}\">", "A".repeat(300 * 1024))).contains("src="));
    }

    #[test]
    fn body_signature_and_quote_are_isolated_and_ordered() {
        let content = MailContent {
            body: "正文\n\n```\n未闭合代码".into(),
            signature: "**签名**\n<span style=\"color:#3F4AAB\">公司</span>".into(),
            quote: Some(MailQuote {
                header: "来源 <person@example.com>".into(), text: "原文 **不是 Markdown**".into(), reply: true, ..Default::default()
            }),
        };
        let out = render(&content);
        assert!(out.html.contains("<strong>签名</strong>"), "{}", out.html);
        assert!(out.html.contains("原文 **不是 Markdown**"));
        assert!(!out.html.contains("<strong>不是 Markdown</strong>"));
        let body = out.html.find("正文").unwrap();
        let sig = out.html.find("签名").unwrap();
        let quote = out.html.find("来源").unwrap();
        assert!(body < sig && sig < quote);
        assert!(out.text.contains("\n\n-- \n签名"), "{}", out.text);
        assert!(out.text.contains("来源 <person@example.com>"));
        assert!(!out.text.contains("<span") && !out.text.contains("<strong"));
    }

    #[test]
    fn original_html_and_entities_are_not_double_escaped() {
        let out = render(&MailContent {
            body: "正文".into(),
            signature: "<b>公司</b>".into(),
            quote: Some(MailQuote {
                header: "发件人：某人 <person@example.com>".into(),
                text: "&nbsp;污染的纯文本".into(),
                html: "<p>您好&nbsp;&nbsp;测试 &gt; A &amp; B <b>原文</b></p>".into(),
                ..Default::default()
            }),
        });
        assert!(out.html.contains("<b>原文</b>"));
        assert!(!out.html.contains("&amp;nbsp;") && !out.html.contains("&amp;gt;"));
        assert!(!out.text.contains("&nbsp;") && !out.text.contains("&gt;") && !out.text.contains("<b>"));
        assert!(out.text.contains("> A & B"), "{}", out.text);
    }

    #[test]
    fn literal_entities_and_code_are_not_recursively_decoded_into_markup() {
        let out = render(&MailContent {
            body: "`<b>源码</b>` 和 `&nbsp;`".into(),
            quote: Some(MailQuote { text: "<b>这是纯文本</b> &nbsp;".into(), ..Default::default() }),
            ..Default::default()
        });
        assert!(out.html.contains("<code>&lt;b&gt;源码&lt;/b&gt;</code>"));
        assert!(out.html.contains("&lt;b&gt;这是纯文本&lt;/b&gt; &amp;nbsp;"));
        assert!(!out.html.contains("<b>这是纯文本</b>"));
        assert!(out.text.contains("<b>源码</b>"));
    }

    #[test]
    fn plain_text_preserves_links_lists_newlines_and_code() {
        let out = render(&MailContent {
            body: "第一行\n第二行\n\n- 项目一\n- 项目二\n\n[查看详情](https://example.com/details)\n\n```\n  code <tag>\n```".into(),
            ..Default::default()
        });
        assert!(out.text.starts_with("第一行\n第二行"), "{}", out.text);
        assert!(out.text.contains("项目一\n") && out.text.contains("项目二"), "{}", out.text);
        assert!(out.text.contains("https://example.com/details"), "{}", out.text);
        assert!(out.text.contains("code <tag>"));
    }

    #[test]
    fn empty_content_does_not_create_signature_rule() {
        for body in ["", "   \n  "] {
            let out = render(&MailContent { body: body.into(), ..Default::default() });
            assert_eq!(out.preview, "");
            assert_eq!(out.text, "");
        }
        let out = render(&MailContent { signature: "**签名**".into(), ..Default::default() });
        assert!(!out.html.contains(SIG_RULE));
        assert_eq!(out.text, "签名");
    }

    #[test]
    fn legacy_drafts_remove_signature_before_quote_or_at_end() {
        let sig = "<b>签名</b>";
        for head in ["---------- 转发的邮件 ----------", "---------- Forwarded message ----------", "On 2026-11-13, A wrote:", "在 2026-11-13，A 写道："]
        {
            let old = format!("正文\n\n-- \n{sig}\n\n{head}\n原文");
            let restored = restore_legacy(&old, sig);
            assert_eq!(restored.body, "正文");
            let rendered = render(&restored);
            assert_eq!(rendered.html.matches("<b>签名</b>").count(), 1, "{}", rendered.html);
            assert!(rendered.html.contains("原文"));
        }
        assert_eq!(restore_legacy(&format!("正文\n\n-- \n{sig}"), sig).body, "正文");
        assert_eq!(restore_legacy(sig, sig).body, "");
        assert_eq!(restore_legacy("正文\r\n\r\n-- \r\n旧签名", "新签名").body, "正文");
        assert_eq!(restore_legacy("正文\n\n-- \n旧签名", "").body, "正文");
    }
}
