//! Kojoo Mail 的 MCP stdio 桥。
//!
//! 只做传输转换：agent 用 stdio 连本进程，本进程用 streamable-HTTP 连桌面端。
//! 工具清单、参数校验、权限判断全在桌面端，这里一行都不复制。
//! 发现文件由桌面端在成功监听后写出、退出时删除；本进程绝不修改设置、绝不自动开启 MCP。

use std::{
    borrow::Cow,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context as _, Result, anyhow, bail};
use rmcp::{
    ClientHandler, ClientLifecycleMode, ClientServiceExt, ErrorData as McpError, RoleClient,
    RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, ClientCapabilities, ClientConfig, Implementation,
        ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerConfig,
    },
    service::{NotificationContext, RequestContext, ServiceError},
    transport::{
        StreamableHttpClientTransport,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// 桌面端只实现到 2025-06-18：之后的协议没有 initialize 握手，通知会静默失效
const MAX_VERSION: ProtocolVersion = ProtocolVersion::V_2025_06_18;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Discovery {
    port: u16,
    url: String,
    token: String,
}

fn discovery_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("KOJOOMAIL_MCP_JSON") {
        return Some(PathBuf::from(p));
    }
    Some(dirs::data_dir()?.join("KojooMail").join("mcp.json"))
}

fn read_discovery() -> Option<Discovery> {
    let raw = std::fs::read_to_string(discovery_path()?).ok()?;
    serde_json::from_str(&raw).ok()
}

/// 不带凭据探一下：只有桌面端会对 /mcp 回 401 Bearer。
/// 应用崩溃后残留的发现文件可能指向别的进程，先确认再发 token。
async fn looks_like_kojoomail(port: u16) -> bool {
    let Ok(Ok(mut sock)) = tokio::time::timeout(
        Duration::from_millis(300),
        tokio::net::TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    else {
        return false;
    };
    let req = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\n\
         Accept: application/json, text/event-stream\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
    );
    if sock.write_all(req.as_bytes()).await.is_err() {
        return false;
    }
    let mut head = [0u8; 64];
    let Ok(Ok(n)) = tokio::time::timeout(Duration::from_millis(500), sock.read(&mut head)).await
    else {
        return false;
    };
    String::from_utf8_lossy(&head[..n]).contains(" 401 ")
}

fn locate_exe() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("KOJOOMAIL_EXE") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let name = if cfg!(windows) { "Kojoo Mail.exe" } else { "Kojoo Mail" };
    let self_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    // 安装目录里代理和主程序同级；开发时代理在 target/release，主程序在上一级
    for dir in [self_dir.clone(), self_dir.parent()?.to_path_buf()] {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    #[cfg(windows)]
    if let Some(local) = dirs::data_local_dir() {
        let p = local.join("Programs").join("Kojoo Mail").join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let p = PathBuf::from("/Applications/Kojoo Mail.app/Contents/MacOS/Kojoo Mail");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// 拉起桌面端（缩到托盘），等它写出发现文件。开启 MCP 与否由用户在界面里决定。
async fn launch_and_wait() -> Result<Discovery> {
    let exe = locate_exe().ok_or_else(|| {
        anyhow!("找不到 Kojoo Mail 主程序。请先安装并启动 Kojoo Mail，或用 KOJOOMAIL_EXE 指定路径。")
    })?;
    tokio::process::Command::new(&exe)
        .arg("--hidden")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("启动 {} 失败", exe.display()))?;

    for _ in 0..150 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if let Some(d) = read_discovery() {
            if looks_like_kojoomail(d.port).await {
                return Ok(d);
            }
        }
    }
    bail!(
        "Kojoo Mail 已启动，但 30 秒内没有提供 MCP 服务。请打开 Kojoo Mail → 设置 → MCP，把开关打开后重试。"
    )
}

async fn discover() -> Result<Discovery> {
    if let Some(d) = read_discovery() {
        if looks_like_kojoomail(d.port).await {
            return Ok(d);
        }
    }
    launch_and_wait().await
}

fn to_mcp(e: ServiceError) -> McpError {
    match e {
        ServiceError::McpError(e) => e,
        other => McpError::internal_error(format!("Kojoo Mail 连接中断：{other}"), None),
    }
}

/// 下游 peer 由 on_initialized 填入，上游收到工具变更时往这里推
#[derive(Clone, Default)]
struct Downstream(Arc<Mutex<Option<rmcp::service::Peer<RoleServer>>>>);

impl Downstream {
    fn set(&self, peer: rmcp::service::Peer<RoleServer>) {
        if let Ok(mut g) = self.0.lock() {
            *g = Some(peer);
        }
    }
    fn get(&self) -> Option<rmcp::service::Peer<RoleServer>> {
        self.0.lock().ok().and_then(|g| g.clone())
    }
}

#[derive(Clone)]
struct Upstream {
    downstream: Downstream,
}

impl ClientHandler for Upstream {
    async fn on_tool_list_changed(&self, _ctx: NotificationContext<RoleClient>) {
        if let Some(peer) = self.downstream.get() {
            let _ = peer.notify_tool_list_changed().await;
        }
    }
}

#[derive(Clone)]
struct Bridge {
    upstream: rmcp::service::Peer<RoleClient>,
    info: ServerConfig,
    downstream: Downstream,
}

impl ServerHandler for Bridge {
    fn get_info(&self) -> ServerConfig {
        self.info.clone()
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(ProtocolVersion::known_up_to(&MAX_VERSION))
    }

    async fn on_initialized(&self, ctx: NotificationContext<RoleServer>) {
        self.downstream.set(ctx.peer.clone());
    }

    async fn list_tools(
        &self,
        params: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        self.upstream.list_tools(params).await.map_err(to_mcp)
    }

    async fn call_tool(
        &self,
        params: CallToolRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        Ok(self.upstream.call_tool(params).await.map_err(to_mcp)?.into())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let d = discover().await?;
    let downstream = Downstream::default();
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(d.url.clone()).auth_header(d.token.clone()),
    );
    let client = ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new("kojoomail-mcp", env!("CARGO_PKG_VERSION")),
    )
    .serve_with_lifecycle(
        transport,
        ClientLifecycleMode::Initialize,
    )
    .await
    .map_err(|e| anyhow!("连接 Kojoo Mail 的 MCP 服务失败：{e}"))?;

    let mut info = client
        .peer_info()
        .map(|i| (*i).clone())
        .unwrap_or_else(|| ServerConfig::new(Default::default()));
    info.protocol_version = MAX_VERSION;
    info.server_info = Implementation::new("kojoomail-mcp", env!("CARGO_PKG_VERSION"));

    let bridge = Bridge {
        upstream: client.peer().clone(),
        info,
        downstream: downstream.clone(),
    };
    let server = bridge
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| anyhow!("启动 stdio 服务失败：{e}"))?;
    let _ = server.waiting().await;
    let _ = client.cancel().await;
    Ok(())
}