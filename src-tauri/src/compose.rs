//! 写信预填：回复 / 全部回复 / 转发的收件人、主题前缀、引用正文和签名。
//!
//! 前端 Compose.tsx 有一份等价逻辑给界面用。两边各自独立：漂移只影响引用排版，
//! threading 头（In-Reply-To / References）由 send_mail_inner 统一处理，不受这里影响。

use std::collections::HashSet;

use rusqlite::OptionalExtension;

use crate::mime::Addr;
use crate::{Ctx, i18n, store::Store};

/// 预填好的一封草稿，字段和 commands::DraftIn 对得上
pub struct Prefill {
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub subject: String,
    pub body: String,
    pub reply_msg: Option<i64>,
    pub mode: String,
}

/// 原邮件里预填要用到的部分。不取 AI 批注和待办，预填用不上
struct Base {
    id: i64,
    account_id: i64,
    from_name: String,
    from_addr: String,
    to: Vec<Addr>,
    cc: Vec<Addr>,
    reply_to: String,
    subject: String,
    date: i64,
    text: String,
}

fn base(st: &Store, id: i64) -> Result<Base, String> {
    let addrs = |s: String| serde_json::from_str::<Vec<Addr>>(&s).unwrap_or_default();
    st.conn
        .query_row(
            "SELECT id, account_id, from_name, from_addr, to_json, cc_json, reply_to, subject, date, body_text FROM messages WHERE id=?",
            [id],
            |r| {
                Ok(Base {
                    id: r.get(0)?,
                    account_id: r.get(1)?,
                    from_name: r.get(2)?,
                    from_addr: r.get(3)?,
                    to: addrs(r.get(4)?),
                    cc: addrs(r.get(5)?),
                    reply_to: r.get(6)?,
                    subject: r.get(7)?,
                    date: r.get(8)?,
                    text: r.get(9)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(crate::mcp_scope::gone)
}

/// `mode` 只认 reply / replyAll / forward。`text` 是 agent 写的新正文，排在引用上面。
/// 调用方负责先判权限、先 ensure_body：这里不查 scope，也不联网拉正文。
pub fn prefill(ctx: &Ctx, base_id: i64, mode: &str, account_id: Option<i64>, text: &str) -> Result<Prefill, String> {
    if !matches!(mode, "reply" | "replyAll" | "forward") {
        return Err(format!("unknown mode: {mode}"));
    }
    let st = ctx.store.lock().unwrap();
    let b = base(&st, base_id)?;
    let account = account_id.unwrap_or(b.account_id);
    let mut q = st.conn.prepare("SELECT email, signature FROM accounts").map_err(|e| e.to_string())?;
    let rows: Vec<(String, String)> = q
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| e.to_string())?
        .filter_map(|x| x.ok())
        .collect();
    // 全部回复要剔掉「自己」：用户的每一个账号地址都算自己，不只是发件那个
    let self_addrs: Vec<String> = rows.iter().map(|(e, _)| e.to_lowercase()).collect();
    let sig = st
        .conn
        .query_row("SELECT signature FROM accounts WHERE id=?", [account], |r| r.get::<_, String>(0))
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    Ok(build(&b, mode, &self_addrs, &sig, text))
}

/// 纯函数，便于穷举测试
fn build(b: &Base, mode: &str, self_addrs: &[String], sig: &str, text: &str) -> Prefill {
    let en = i18n::en();
    let who = if b.from_name.is_empty() { b.from_addr.clone() } else { format!("{} <{}>", b.from_name, b.from_addr) };
    let mut to: Vec<Addr> = vec![];
    let mut cc: Vec<Addr> = vec![];
    let subject;
    let quote;
    if mode == "forward" {
        subject = forward_subject(&b.subject);
        quote = if en {
            format!("\n\n---------- Forwarded message ----------\nFrom: {who}\nDate: {}\nSubject: {}\n\n{}", full_time(b.date), b.subject, b.text)
        } else {
            format!("\n\n---------- 转发的邮件 ----------\n发件人：{who}\n时间：{}\n主题：{}\n\n{}", full_time(b.date), b.subject, b.text)
        };
    } else {
        // Reply-To 优先：邮件列表和「请勿回复」的信都靠它把回复引到别处
        to = vec![if b.reply_to.is_empty() {
            Addr { name: b.from_name.clone(), email: b.from_addr.clone() }
        } else {
            Addr { name: String::new(), email: b.reply_to.clone() }
        }];
        if mode == "replyAll" {
            let me: HashSet<String> = self_addrs.iter().cloned().collect();
            let mut seen: HashSet<String> = to.iter().map(|a| a.email.to_lowercase()).collect();
            cc = b
                .to
                .iter()
                .chain(b.cc.iter())
                .filter(|a| {
                    let k = a.email.to_lowercase();
                    if me.contains(&k) || seen.contains(&k) {
                        return false;
                    }
                    seen.insert(k);
                    true
                })
                .cloned()
                .collect();
        }
        subject = reply_subject(&b.subject);
        let header = if en { format!("On {}, {who} wrote:", full_time(b.date)) } else { format!("在 {}，{who} 写道：", full_time(b.date)) };
        quote = quote_text(&b.text, &header);
    }
    Prefill {
        to,
        cc,
        subject,
        body: format!("{}{quote}", with_signature(text, sig)),
        reply_msg: Some(b.id),
        mode: mode.into(),
    }
}

/// 前缀幂等：Re: Re: 这种叠罗汉不要
fn reply_subject(s: &str) -> String {
    if s.to_ascii_lowercase().starts_with("re:") { s.to_string() } else { format!("Re: {s}") }
}

fn forward_subject(s: &str) -> String {
    let l = s.to_ascii_lowercase();
    if l.starts_with("fwd:") || l.starts_with("fw:") || l.starts_with("转发:") { s.to_string() } else { format!("Fwd: {s}") }
}

/// 逐行 `> `，最多 200 行：长回复链能省掉绝大部分 token
fn quote_text(text: &str, header: &str) -> String {
    let lines: Vec<String> = text.split('\n').take(200).map(|l| format!("> {}", l.trim_end_matches('\r'))).collect();
    format!("\n\n{header}\n{}", lines.join("\n"))
}

/// 和前端 md.ts withSignature 一致：纯文本用 RFC 3676 的 "-- " 分隔行
fn with_signature(body: &str, sig: &str) -> String {
    if sig.is_empty() {
        return body.to_string();
    }
    if body.trim().is_empty() {
        return sig.to_string();
    }
    format!("{body}\n\n-- \n{sig}")
}

/// 和前端 format.ts fullTime 一致：本地时间 YYYY-MM-DD HH:MM
fn full_time(ts: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| ts.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Base {
        Base {
            id: 7,
            account_id: 1,
            from_name: "张三".into(),
            from_addr: "zhang@supplier.com".into(),
            to: vec![Addr { name: "我".into(), email: "me@example.com".into() }, Addr { name: "同事".into(), email: "colleague@example.com".into() }],
            cc: vec![Addr { name: "老板".into(), email: "boss@example.com".into() }, Addr { name: "我".into(), email: "ME@example.com".into() }],
            reply_to: String::new(),
            subject: "询价：FOB 上海".into(),
            date: 1_700_000_000,
            text: "请报价。\n交期多久？".into(),
        }
    }

    fn me() -> Vec<String> {
        vec!["me@example.com".into(), "other@example.com".into()]
    }

    #[test]
    fn subject_prefixes_are_idempotent() {
        assert_eq!(reply_subject("询价"), "Re: 询价");
        assert_eq!(reply_subject("Re: 询价"), "Re: 询价");
        assert_eq!(reply_subject("RE: 询价"), "RE: 询价");
        assert_eq!(reply_subject("re: 询价"), "re: 询价");
        assert_eq!(forward_subject("询价"), "Fwd: 询价");
        assert_eq!(forward_subject("Fwd: 询价"), "Fwd: 询价");
        assert_eq!(forward_subject("FW: 询价"), "FW: 询价");
        assert_eq!(forward_subject("fwd: 询价"), "fwd: 询价");
        assert_eq!(forward_subject("转发: 询价"), "转发: 询价");
        // 只是开头长得像，不算前缀
        assert_eq!(reply_subject("Reply: 询价"), "Re: Reply: 询价");
        assert_eq!(forward_subject("Fwdd: 询价"), "Fwd: Fwdd: 询价");
    }

    #[test]
    fn reply_goes_to_reply_to_when_present() {
        let mut b = base();
        b.reply_to = "list@supplier.com".into();
        let p = build(&b, "reply", &me(), "", "好的");
        assert_eq!(p.to.len(), 1);
        assert_eq!(p.to[0].email, "list@supplier.com");
        assert!(p.cc.is_empty());
        assert_eq!(p.reply_msg, Some(7));
        assert_eq!(p.mode, "reply");
    }

    #[test]
    fn reply_falls_back_to_from() {
        let p = build(&base(), "reply", &me(), "", "好的");
        assert_eq!(p.to[0].email, "zhang@supplier.com");
        assert_eq!(p.to[0].name, "张三");
        assert!(p.cc.is_empty());
    }

    /// 全部回复：剔掉自己的每个地址、剔掉已在收件人里的、大小写不敏感、保持顺序
    #[test]
    fn reply_all_drops_self_and_duplicates() {
        let p = build(&base(), "replyAll", &me(), "", "好的");
        assert_eq!(p.to[0].email, "zhang@supplier.com");
        let cc: Vec<&str> = p.cc.iter().map(|a| a.email.as_str()).collect();
        assert_eq!(cc, vec!["colleague@example.com", "boss@example.com"]);
    }

    #[test]
    fn reply_all_drops_every_own_account() {
        let mut b = base();
        b.cc.push(Addr { name: String::new(), email: "other@example.com".into() });
        let p = build(&b, "replyAll", &me(), "", "好的");
        assert!(!p.cc.iter().any(|a| a.email.eq_ignore_ascii_case("other@example.com")));
    }

    #[test]
    fn forward_has_no_recipients_and_keeps_original_subject_in_header() {
        let p = build(&base(), "forward", &me(), "", "转给你");
        assert!(p.to.is_empty());
        assert!(p.cc.is_empty());
        assert_eq!(p.subject, "Fwd: 询价：FOB 上海");
        assert!(p.body.contains("zhang@supplier.com"));
        assert!(p.body.contains("询价：FOB 上海"));
        assert!(p.body.contains("请报价。"));
        assert_eq!(p.reply_msg, Some(7));
    }

    #[test]
    fn quote_prefixes_every_line_and_caps_at_200() {
        let long = (0..500).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let q = quote_text(&long, "H");
        assert!(q.starts_with("\n\nH\n"));
        let lines: Vec<&str> = q.trim_start_matches("\n\nH\n").split('\n').collect();
        assert_eq!(lines.len(), 200);
        assert!(lines.iter().all(|l| l.starts_with("> ")));
        assert_eq!(lines[199], "> line 199");
        assert!(!q.contains("line 200"));
    }

    #[test]
    fn quote_handles_crlf() {
        let q = quote_text("a\r\nb\r\n", "H");
        assert!(q.contains("> a\n> b\n> "));
        assert!(!q.contains('\r'));
    }

    #[test]
    fn signature_uses_rfc3676_separator() {
        assert_eq!(with_signature("正文", "张三"), "正文\n\n-- \n张三");
        assert_eq!(with_signature("正文", ""), "正文");
        assert_eq!(with_signature("", "张三"), "张三");
        assert_eq!(with_signature("   ", "张三"), "张三");
    }

    /// 正文 → 签名 → 引用，顺序和前端 Compose.tsx 一致
    #[test]
    fn body_order_is_text_then_signature_then_quote() {
        let p = build(&base(), "reply", &me(), "张三", "报 $12.5");
        let i = p.body.find("报 $12.5").unwrap();
        let j = p.body.find("-- \n张三").unwrap();
        let k = p.body.find("> 请报价。").unwrap();
        assert!(i < j && j < k, "{}", p.body);
    }

    #[test]
    fn empty_text_still_quotes_the_original() {
        let p = build(&base(), "reply", &me(), "", "");
        assert!(p.body.starts_with("\n\n"));
        assert!(p.body.contains("> 请报价。"));
    }
}