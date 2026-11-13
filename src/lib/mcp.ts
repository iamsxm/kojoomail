// MCP 配置片段：同一个 stdio 服务，只是各客户端配置文件的写法不同。
// 协议本身是通用 MCP，没有针对任何客户端的适配。

/** Claude Code / Cursor / Qoder 等用的 mcpServers JSON */
export function mcpJson(proxyPath: string): string {
  return JSON.stringify({ mcpServers: { kojoomail: { command: proxyPath, args: [] } } }, null, 2)
}

/** Codex 的 config.toml；用基本字符串，反斜杠和引号都要转义 */
export function mcpToml(proxyPath: string): string {
  const esc = proxyPath.replace(/\\/g, '\\\\').replace(/"/g, '\\"')
  return `[mcp_servers.kojoomail]\ncommand = "${esc}"\nargs = []\n`
}

/** 复制配置。WebView2 里 navigator.clipboard 可能没权限，退回选中 textarea 的老办法 */
export async function copyText(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text)
      return true
    }
  } catch {
    // 落到下面
  }
  const ta = document.createElement('textarea')
  ta.value = text
  ta.setAttribute('readonly', '')
  ta.style.position = 'fixed'
  ta.style.opacity = '0'
  document.body.appendChild(ta)
  ta.select()
  let ok = false
  try {
    ok = document.execCommand('copy')
  } catch {
    ok = false
  }
  document.body.removeChild(ta)
  return ok
}