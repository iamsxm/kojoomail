//! 发信：mail-builder 组信，lettre 走 SMTP（465 直连 TLS / 587 STARTTLS）。
//! 连接自己建（跟随系统代理，见 net.rs），再交给 lettre 跑 SMTP 协议。

use crate::mime::Addr;
use anyhow::{anyhow, Result};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::transport::smtp::client::{AsyncSmtpConnection, AsyncTokioStream, TlsParameters};
use lettre::transport::smtp::extension::ClientId;
use mail_builder::headers::address::Address as MbAddress;
use mail_builder::MessageBuilder;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

pub struct SmtpCfg {
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub username: String,
    /// 密码/授权码；一键登录的账号是短期令牌
    pub password: String,
    pub oauth: bool,
}

pub struct OutAttachment {
    pub name: String,
    pub mime: String,
    pub data: Vec<u8>,
}

pub struct Outgoing {
    pub from: Addr,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub bcc: Vec<Addr>,
    pub subject: String,
    pub text: String,
    pub html: String,
    pub in_reply_to: String,
    pub references: String,
    pub attachments: Vec<OutAttachment>,
}

/// 465 端口：TLS 由我们先握手，再交给 lettre
struct TlsConn(tokio_native_tls::TlsStream<TcpStream>, std::net::SocketAddr);

impl std::fmt::Debug for TlsConn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TlsConn({})", self.1)
    }
}

impl AsyncRead for TlsConn {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for TlsConn {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

impl AsyncTokioStream for TlsConn {
    fn peer_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        Ok(self.1)
    }
}

fn no_response() -> &'static str {
    crate::i18n::t(NO_RESP_ZH, NO_RESP_EN)
}
const NO_RESP_ZH: &str = "发信服务器没有响应：";
const NO_RESP_EN: &str = "The outgoing server didn't respond: ";

/// 连上、加密、登录
async fn open(c: &SmtpCfg) -> Result<AsyncSmtpConnection> {
    let (tcp, via_proxy) = crate::net::dial(&c.host, c.port).await.map_err(|e| anyhow!(tr!("连不上发信服务器：{e}", "Can't reach the outgoing mail server: {e}")))?;
    let mut conn = if via_proxy {
        // 不少代理节点封 SMTP 端口（iCloud 的 587 就是）：隧道建好了却收不到欢迎语，或者一直没动静。
        // 只有这两种情况改直连再试；证书错、不支持 STARTTLS 之类直连也一样，照原样报
        let first = match tokio::time::timeout(Duration::from_secs(15), handshake(c, tcp)).await {
            Ok(Ok(conn)) => Ok(conn),
            Ok(Err(x)) if !crate::i18n::starts_with_any(&x.to_string(), NO_RESP_ZH, NO_RESP_EN) => return Err(x),
            Ok(Err(x)) => Err(x.to_string()),
            Err(_) => Err(crate::i18n::t("15 秒没有响应", "no response in 15 seconds").to_string()),
        };
        match first {
            Ok(conn) => conn,
            Err(first) => {
                let fail = |x: String| anyhow!(tr!("经代理连发信服务器失败（{first}），改直连也失败：{x}", "Connecting to the outgoing server through the proxy failed ({first}), and a direct connection also failed: {x}"));
                let tcp = crate::net::dial_direct(&c.host, c.port).await.map_err(fail)?;
                timed(20, handshake(c, tcp)).await.map_err(|x| fail(x.to_string()))?
            }
        }
    } else {
        handshake(c, tcp).await?
    };
    login(c, &mut conn).await?;
    Ok(conn)
}

/// 收欢迎语、加密（465 直连 TLS；其他端口必须 STARTTLS，不发明文）
async fn handshake(c: &SmtpCfg, tcp: TcpStream) -> Result<AsyncSmtpConnection> {
    let peer = tcp.peer_addr()?;
    let hello = ClientId::default();
    let conn = if c.tls && c.port == 465 {
        let cx = native_tls::TlsConnector::new().map_err(|e| anyhow!(tr!("TLS 初始化失败：{e}", "TLS setup failed: {e}")))?;
        let tls = tokio_native_tls::TlsConnector::from(cx).connect(&c.host, tcp).await.map_err(|e| anyhow!(tr!("发信服务器 TLS 握手失败：{e}", "TLS handshake with the outgoing server failed: {e}")))?;
        AsyncSmtpConnection::connect_with_transport(Box::new(TlsConn(tls, peer)), &hello).await.map_err(|e| anyhow!("{}{e}", no_response()))?
    } else {
        let mut conn = AsyncSmtpConnection::connect_with_transport(Box::new(tcp), &hello).await.map_err(|e| anyhow!("{}{e}", no_response()))?;
        if !conn.can_starttls() {
            return Err(anyhow!("{}", crate::i18n::t("发信服务器不支持加密连接（STARTTLS），为了安全不发送", "The outgoing server doesn't support an encrypted connection (STARTTLS), so nothing was sent")));
        }
        let params = TlsParameters::new(c.host.clone()).map_err(|e| anyhow!(tr!("TLS 初始化失败：{e}", "TLS setup failed: {e}")))?;
        conn.starttls(params, &hello).await.map_err(|e| anyhow!(tr!("发信服务器 TLS 握手失败：{e}", "TLS handshake with the outgoing server failed: {e}")))?;
        conn
    };
    Ok(conn)
}

async fn login(c: &SmtpCfg, conn: &mut AsyncSmtpConnection) -> Result<()> {
    if c.oauth {
        // XOAUTH2 自己发：令牌被拒时服务器先回 334 + 错误说明，要回一个空行才拿到真正的失败原因（lettre 遇到 334 只会报看不懂的错）
        use base64::Engine;
        let ir = base64::engine::general_purpose::STANDARD.encode(format!("user={}\x01auth=Bearer {}\x01\x01", c.username, c.password));
        let r = conn.command(format!("AUTH XOAUTH2 {ir}\r\n")).await.map_err(|e| anyhow!("{}{e}", auth_fail()))?;
        if !r.has_code(235) {
            let e = match conn.command("\r\n").await {
                Err(e) => e.to_string(),
                Ok(r) => r.message().collect::<Vec<_>>().join(" "),
            };
            return Err(anyhow!("{}{e}", auth_fail()));
        }
    } else {
        conn.auth(&[Mechanism::Plain, Mechanism::Login], &Credentials::new(c.username.clone(), c.password.clone())).await.map_err(|e| anyhow!("{}{e}", auth_fail()))?;
    }
    Ok(())
}

/// 登录失败的错误都以这个开头（发信时据此判断要不要换个令牌重试）
/// 报错前缀：上层靠它认出「登录失败」
pub fn auth_fail() -> &'static str {
    crate::i18n::t(AUTH_FAIL_ZH, AUTH_FAIL_EN)
}
const AUTH_FAIL_ZH: &str = "发信服务器登录失败：";
const AUTH_FAIL_EN: &str = "Sign-in to the outgoing server failed: ";
pub fn is_auth_fail(e: &str) -> bool {
    crate::i18n::starts_with_any(e, AUTH_FAIL_ZH, AUTH_FAIL_EN)
}

/// 收尾：服务器不回 QUIT 也别卡住（信已经发出去了）
async fn quit(mut conn: AsyncSmtpConnection) {
    let _ = tokio::time::timeout(Duration::from_secs(5), conn.quit()).await;
}

async fn timed<T>(secs: u64, f: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(Duration::from_secs(secs), f).await.map_err(|_| anyhow!("{}", crate::i18n::t("发信服务器响应超时", "The outgoing server timed out")))?
}

/// 连上到登录完的总时限：代理那条路最多约 40 秒，还要留出改直连的时间
const OPEN_SECS: u64 = 90;

/// 附件大时上传慢，按大小给时间（至少 1 分钟，每 MB 再加 20 秒）
pub fn upload_secs(len: usize) -> u64 {
    60 + (len as u64 / 1_000_000) * 20
}

pub async fn test(c: &SmtpCfg) -> Result<()> {
    let conn = timed(OPEN_SECS, open(c)).await?;
    quit(conn).await;
    Ok(())
}

/// 组好原始邮件。SMTP 发信不带 Bcc 头（密送人只走信封）；Graph 发信没有信封，靠 Bcc 头给密送人，Graph 发出时会去掉
pub fn build(o: &Outgoing, domain: &str, bcc_header: bool) -> Result<Vec<u8>> {
    let list = |v: &Vec<Addr>| MbAddress::new_list(v.iter().map(|a| MbAddress::new_address(if a.name.is_empty() { None } else { Some(a.name.clone()) }, a.email.clone())).collect());
    let msgid = format!("{}.{}@{}", chrono::Utc::now().timestamp_millis(), std::process::id(), domain);
    let mut b = MessageBuilder::new()
        .from(MbAddress::new_address(if o.from.name.is_empty() { None } else { Some(o.from.name.clone()) }, o.from.email.clone()))
        .to(list(&o.to))
        .subject(o.subject.clone())
        .message_id(msgid)
        .text_body(o.text.clone())
        .header("X-Mailer", mail_builder::headers::raw::Raw::new("Kojoo Mail"));
    if !o.cc.is_empty() {
        b = b.cc(list(&o.cc));
    }
    if bcc_header && !o.bcc.is_empty() {
        b = b.bcc(list(&o.bcc));
    }
    if !o.html.is_empty() {
        b = b.html_body(o.html.clone());
    }
    if !o.in_reply_to.is_empty() {
        b = b.in_reply_to(o.in_reply_to.trim_matches(|c| c == '<' || c == '>').to_string());
    }
    let refs: Vec<String> = o.references.split_whitespace().map(|s| s.trim_matches(|c| c == '<' || c == '>').to_string()).filter(|s| !s.is_empty()).collect();
    if !refs.is_empty() {
        b = b.references(refs);
    }
    for a in &o.attachments {
        b = b.attachment(a.mime.clone(), a.name.clone(), a.data.clone());
    }
    Ok(b.write_to_vec()?)
}

/// 信封：发件人和全部收件人（含密送），顺带检查地址格式。Graph 发信不用信封，但也先过这一遍
pub fn envelope(o: &Outgoing) -> Result<lettre::address::Envelope> {
    let from: lettre::Address = o.from.email.parse().map_err(|_| anyhow!(tr!("发件地址不对：{}", "Invalid sender address: {}", o.from.email)))?;
    let mut rcpt = vec![];
    for a in o.to.iter().chain(&o.cc).chain(&o.bcc) {
        rcpt.push(a.email.parse::<lettre::Address>().map_err(|_| anyhow!(tr!("收件地址不对：{}", "Invalid recipient address: {}", a.email)))?);
    }
    if rcpt.is_empty() {
        return Err(anyhow!("{}", crate::i18n::t("没有收件人", "No recipients")));
    }
    lettre::address::Envelope::new(Some(from), rcpt).map_err(|e| anyhow!("{e}"))
}

pub async fn send(c: &SmtpCfg, o: &Outgoing, raw: &[u8]) -> Result<()> {
    let env = envelope(o)?;
    let mut conn = timed(OPEN_SECS, open(c)).await?;
    timed(upload_secs(raw.len()), async { conn.send(&env, raw).await.map_err(|e| anyhow!(tr!("发送失败：{e}", "Sending failed: {e}"))) }).await?;
    quit(conn).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SMTP 版不带 Bcc 头（密送人只走信封）；Graph 版要带，Graph 靠它投递给密送人
    #[test]
    fn bcc_header_only_for_graph() {
        let a = |e: &str| Addr { name: String::new(), email: e.into() };
        let o = Outgoing {
            from: a("me@outlook.com"),
            to: vec![a("to@example.com")],
            cc: vec![],
            bcc: vec![a("hidden@example.com")],
            subject: "s".into(),
            text: "t".into(),
            html: String::new(),
            in_reply_to: String::new(),
            references: String::new(),
            attachments: vec![],
        };
        let smtp = String::from_utf8(build(&o, "outlook.com", false).unwrap()).unwrap();
        let graph = String::from_utf8(build(&o, "outlook.com", true).unwrap()).unwrap();
        assert!(!smtp.contains("hidden@example.com"));
        assert!(graph.contains("Bcc: <hidden@example.com>") || graph.contains("Bcc: hidden@example.com"), "{graph}");
    }

    /// 回复的 threading 头。send_mail_inner 用 `format!("{refs} {mid}")` 拼 References，
    /// 转发时两个都留空——这里按同样的输入验 MIME 里真的带上了，且尖括号不重复
    #[test]
    fn reply_carries_threading_headers_and_forward_does_not() {
        let a = |e: &str| Addr { name: String::new(), email: e.into() };
        let mut o = Outgoing {
            from: a("me@example.com"),
            to: vec![a("them@vendor.com")],
            cc: vec![],
            bcc: vec![],
            subject: "Re: 询价".into(),
            text: "报价如下".into(),
            html: String::new(),
            in_reply_to: "<m1@vendor.com>".into(),
            references: "<m0@vendor.com> <m1@vendor.com>".into(),
            attachments: vec![],
        };
        let reply = String::from_utf8(build(&o, "example.com", false).unwrap()).unwrap();
        let header = |name: &str| {
            reply
                .lines()
                .find(|l| l.to_ascii_lowercase().starts_with(&format!("{name}:")))
                .unwrap_or_else(|| panic!("没有 {name} 头：\n{reply}"))
                .to_string()
        };
        let irt = header("in-reply-to");
        assert!(irt.contains("m1@vendor.com"), "{irt}");
        assert!(!irt.contains("<<") && !irt.contains(">>"), "{irt}");
        let refs = header("references");
        assert!(refs.find("m0@vendor.com").unwrap() < refs.find("m1@vendor.com").unwrap(), "{refs}");
        assert!(!refs.contains("<<") && !refs.contains(">>"), "{refs}");

        o.in_reply_to = String::new();
        o.references = String::new();
        let fwd = String::from_utf8(build(&o, "example.com", false).unwrap()).unwrap();
        assert!(!fwd.to_ascii_lowercase().contains("in-reply-to:"), "{fwd}");
        assert!(!fwd.to_ascii_lowercase().contains("references:"), "{fwd}");
    }
}
