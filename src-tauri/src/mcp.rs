//! MCP 服务端：让本机的通用 AI agent 读邮件。默认关闭，关着的时候连端口都不 bind。
//!
//! 只在 127.0.0.1 上监听，三层防护：绑定回环地址 + 校验 Host（防 DNS rebinding）+ 拒绝一切带
//! Origin 的请求（浏览器必带，原生 MCP 客户端不带，挡掉恶意网页）。每次请求还要对 Bearer token。
//!
//! 没有任何定时器：rmcp 默认的 15 秒 SSE 心跳显式关掉，空闲时不占 CPU（CONTRIBUTING.md 的硬要求）。

use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use bytes::Bytes;
use http::{Request, Response, StatusCode, header};
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tokio_util::sync::CancellationToken;
use tower_service::Service;

use crate::{Ctx, applog, settings};

/// 只实现到 2025-06-18：之后的协议没有 initialize 握手，on_initialized 不触发，
/// 工具清单变更通知会静默失效
pub const MAX_VERSION: rmcp::model::ProtocolVersion = rmcp::model::ProtocolVersion::V_2025_06_18;

type Boxed = BoxBody<Bytes, std::convert::Infallible>;
type Svc = StreamableHttpService<crate::mcp_tools::Mail, LocalSessionManager>;

struct Running {
    port: u16,
    /// 设置里要求的端口，可能和实际绑到的不一样（被占时回落随机）
    wanted: u16,
    /// 换令牌时原地改这里，不重启监听
    token: Arc<Mutex<String>>,
    ct: CancellationToken,
    accept: tauri::async_runtime::JoinHandle<()>,
    shared: crate::mcp_tools::Shared,
}

static STATE: OnceLock<Mutex<Option<Running>>> = OnceLock::new();
fn state() -> &'static Mutex<Option<Running>> {
    STATE.get_or_init(|| Mutex::new(None))
}

/// 设置或账号授权变了就调这里：该开就开、该关就关，开着就重算工具清单并通知已连上的 agent。
/// 同步函数：监听用的是 std 的 TcpListener，不需要在调用点 await。
pub fn refresh(ctx: &Arc<Ctx>) {
    let s = settings::load(ctx);
    let mut g = state().lock().unwrap();
    if !s.mcp_enabled {
        if let Some(r) = g.take() {
            shutdown(r);
        }
        return;
    }
    if let Some(r) = g.as_ref() {
        // 端口改了要重新 bind；没改就只重算工具清单
        if r.wanted == s.mcp_port {
            crate::mcp_tools::apply_permissions(&r.shared, ctx);
            notify_all(&r.shared);
            return;
        }
    }
    if let Some(r) = g.take() {
        shutdown(r);
    }
    match start(ctx, &s) {
        Ok(r) => {
            applog::info(&format!("MCP 已开启：http://127.0.0.1:{}/mcp", r.port));
            *g = Some(r);
        }
        Err(e) => applog::warn(&format!("MCP 启动失败：{e}")),
    }
}

/// 退出时关掉监听并删掉发现文件，免得代理连到一个已经不存在的端口
pub fn stop() {
    if let Some(r) = state().lock().unwrap().take() {
        shutdown(r);
    }
}

fn shutdown(r: Running) {
    r.ct.cancel();
    r.accept.abort();
    let _ = std::fs::remove_file(discovery_path());
}

fn notify_all(shared: &crate::mcp_tools::Shared) {
    for peer in shared.peers() {
        tauri::async_runtime::spawn(async move {
            let _ = peer.notify_tool_list_changed().await;
        });
    }
}

fn start(ctx: &Arc<Ctx>, s: &settings::Settings) -> Result<Running, String> {
    // 上次崩溃留下的发现文件可能指向别的进程，先删掉
    let _ = std::fs::remove_file(discovery_path());
    let token = load_or_new_token()?;
    let std_listener = bind(s.mcp_port)?;
    let port = std_listener.local_addr().map_err(|e| e.to_string())?.port();
    std_listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    // Tauri 的 setup 跑在主线程上，不在 tokio 反应器里；from_std 和 rmcp 内部的 spawn 都要先进上下文
    let _reactor = tauri::async_runtime::handle().inner().enter();
    let listener = tokio::net::TcpListener::from_std(std_listener).map_err(|e| e.to_string())?;
    let token = Arc::new(Mutex::new(token));

    let ct = CancellationToken::new();
    let shared = crate::mcp_tools::Shared::new(ctx.clone());
    crate::mcp_tools::apply_permissions(&shared, ctx);

    let cfg = StreamableHttpServerConfig::default()
        // 常驻心跳定时器，本项目不允许
        .with_sse_keep_alive(None)
        .with_sse_retry(None)
        .with_cancellation_token(ct.clone())
        // 空白名单 + 强制校验 = 拒绝一切带 Origin 的请求
        .enforce_origin_validation();
    let factory_shared = shared.clone();
    let svc: Svc = StreamableHttpService::new(
        move || Ok(crate::mcp_tools::Mail::new(factory_shared.clone())),
        LocalSessionManager::default().into(),
        cfg,
    );

    let accept = tauri::async_runtime::spawn(accept_loop(listener, svc, token.clone()));
    write_discovery(port, &token.lock().unwrap())?;
    Ok(Running { port, wanted: s.mcp_port, token, ct, accept, shared })
}

/// 优先端口被占就回落随机端口：实际端口写进发现文件，对 stdio 用户完全透明
fn bind(preferred: u16) -> Result<std::net::TcpListener, String> {
    let loopback = std::net::SocketAddr::from(([127, 0, 0, 1], preferred));
    match std::net::TcpListener::bind(loopback) {
        Ok(l) => Ok(l),
        Err(e) => {
            if preferred == 0 {
                return Err(e.to_string());
            }
            applog::warn(&format!("MCP 端口 {preferred} 被占用（{e}），改用随机端口"));
            std::net::TcpListener::bind(std::net::SocketAddr::from(([127, 0, 0, 1], 0))).map_err(|e| e.to_string())
        }
    }
}

async fn accept_loop(listener: tokio::net::TcpListener, svc: Svc, token: Arc<Mutex<String>>) {
    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        let _ = stream.set_nodelay(true);
        let svc = svc.clone();
        let token = token.clone();
        tauri::async_runtime::spawn(async move {
            let service = hyper::service::service_fn(move |req: Request<Incoming>| {
                let mut svc = svc.clone();
                // 每个请求现取：换了令牌，连着的 keep-alive 连接下一个请求就得重新认证
                let token = token.lock().unwrap().clone();
                async move { Ok::<_, std::convert::Infallible>(route(&mut svc, &token, req).await) }
            });
            let _ = hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(stream), service).await;
        });
    }
}

/// 鉴权在这里做完再进 rmcp：token 不对的请求连 JSON-RPC 都不解析
async fn route(svc: &mut Svc, token: &str, req: Request<Incoming>) -> Response<Boxed> {
    if req.uri().path() != "/mcp" {
        drain(req.into_body()).await;
        return plain(StatusCode::NOT_FOUND, "not found");
    }
    if !authorized(req.headers(), token) {
        drain(req.into_body()).await;
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(header::WWW_AUTHENTICATE, "Bearer")
            .body(full(""))
            .unwrap_or_else(|_| plain(StatusCode::UNAUTHORIZED, ""));
    }
    match svc.call(req).await {
        Ok(r) => r,
        Err(e) => match e {},
    }
}

/// 回错误前把请求体读掉：还有没收完的数据就关连接，Windows 会发 RST，
/// 客户端看到的是「连接中断」而不是 401。上限 1 MiB，不给本机进程拿内存的机会。
async fn drain(mut body: Incoming) {
    let mut n = 0usize;
    while n < 1 << 20 {
        match body.frame().await {
            Some(Ok(f)) => n += f.data_ref().map(|d| d.len()).unwrap_or(0),
            _ => break,
        }
    }
}

fn authorized(headers: &header::HeaderMap, token: &str) -> bool {
    let Some(v) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) else { return false };
    let Some(got) = v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")) else { return false };
    ct_eq(got.trim().as_bytes(), token.as_bytes())
}

/// 定长比较：不用 == ，免得按字节短路泄露 token 前缀
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn full(s: impl Into<Bytes>) -> Boxed {
    Full::new(s.into()).map_err(|e| match e {}).boxed()
}

fn plain(status: StatusCode, body: &'static str) -> Response<Boxed> {
    Response::builder().status(status).body(full(body)).unwrap_or_else(|_| Response::new(full("")))
}

// ---------------- token 与发现文件 ----------------

fn discovery_path() -> std::path::PathBuf {
    crate::data_dir().join("mcp.json")
}

/// token 单独存一个文件：发现文件关掉时要删，token 要留着复用，不然每次开关都得重配 agent。
/// 和 kojoomail.db 同目录同权限（那里明文存着全部邮件正文），不降低实际安全边界。
fn token_path() -> std::path::PathBuf {
    crate::data_dir().join("mcp-token")
}

fn valid_token(s: &str) -> bool {
    s.len() >= 32 && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

fn load_or_new_token() -> Result<String, String> {
    if let Ok(s) = std::fs::read_to_string(token_path()) {
        let s = s.trim().to_string();
        if valid_token(&s) {
            return Ok(s);
        }
    }
    let t = new_token();
    save_token(&t)?;
    Ok(t)
}

fn save_token(t: &str) -> Result<(), String> {
    let dir = crate::data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(token_path(), t).map_err(|e| e.to_string())
}

fn new_token() -> String {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).expect("系统随机数不可用");
    B64.encode(b)
}

/// 设置页点「重新生成」：已配置的 agent 会失效，需要重新复制配置。
/// 不重启监听：abort 是异步的，旧 listener 还没释放就重新 bind 会撞 10048，端口漂到随机值。
pub fn regenerate_token() -> Result<(), String> {
    let t = new_token();
    save_token(&t)?;
    if let Some(r) = state().lock().unwrap().as_ref() {
        *r.token.lock().unwrap() = t.clone();
        write_discovery(r.port, &t)?;
    }
    Ok(())
}

fn write_discovery(port: u16, token: &str) -> Result<(), String> {
    let v = serde_json::json!({
        "schema": 1,
        "pid": std::process::id(),
        "port": port,
        "url": format!("http://127.0.0.1:{port}/mcp"),
        "token": token,
        "version": env!("CARGO_PKG_VERSION"),
        "startedAt": crate::store::now(),
    });
    std::fs::write(discovery_path(), serde_json::to_string_pretty(&v).unwrap_or_default()).map_err(|e| e.to_string())
}

/// 设置页展示用：连接地址、token、stdio 代理路径。没开启时 port/url 为空
pub fn info() -> serde_json::Value {
    let g = state().lock().unwrap();
    let port = g.as_ref().map(|r| r.port);
    let token = std::fs::read_to_string(token_path()).ok().map(|s| s.trim().to_string()).filter(|s| valid_token(s));
    serde_json::json!({
        "running": port.is_some(),
        "port": port,
        "url": port.map(|p| format!("http://127.0.0.1:{p}/mcp")),
        "token": token,
        "proxyPath": proxy_path(),
    })
}

/// stdio 代理和主程序装在同一个目录（tauri 的 externalBin）
fn proxy_path() -> Option<String> {
    let name = if cfg!(windows) { "kojoomail-mcp.exe" } else { "kojoomail-mcp" };
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let p = dir.join(name);
    p.is_file().then(|| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_compare_rejects_wrong_and_short() {
        let t = "abcdefghijklmnopqrstuvwxyz0123456789_-ABCD";
        assert!(ct_eq(t.as_bytes(), t.as_bytes()));
        assert!(!ct_eq(t.as_bytes(), b"abcdefghijklmnopqrstuvwxyz0123456789_-ABCE"));
        assert!(!ct_eq(t.as_bytes(), b"short"));
        assert!(!ct_eq(t.as_bytes(), b""));
    }

    #[test]
    fn only_bearer_tokens_are_accepted() {
        let t = "abcdefghijklmnopqrstuvwxyz0123456789_-ABCD";
        let mut h = header::HeaderMap::new();
        assert!(!authorized(&h, t));
        h.insert(header::AUTHORIZATION, format!("Bearer {t}").parse().unwrap());
        assert!(authorized(&h, t));
        h.insert(header::AUTHORIZATION, format!("Basic {t}").parse().unwrap());
        assert!(!authorized(&h, t));
        h.insert(header::AUTHORIZATION, "Bearer wrong".parse().unwrap());
        assert!(!authorized(&h, t));
    }

    #[test]
    fn generated_tokens_are_url_safe_and_long() {
        let t = new_token();
        assert!(valid_token(&t));
        assert_ne!(t, new_token());
    }
}