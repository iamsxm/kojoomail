// MCP 配置片段生成 + 复制降级。跑法：npm test
import { test, before } from 'node:test'
import assert from 'node:assert/strict'
import { JSDOM } from 'jsdom'

let mcpJson, mcpToml, copyText

// Node 24 的 globalThis.navigator 只有 getter，只能重新定义
const setNav = (v) => Object.defineProperty(globalThis, 'navigator', { value: v, configurable: true, writable: true })

before(async () => {
  const dom = new JSDOM('<!doctype html><html><body></body></html>')
  globalThis.window = dom.window
  globalThis.document = dom.window.document
  setNav(dom.window.navigator)
  ;({ mcpJson, mcpToml, copyText } = await import('../src/lib/mcp.ts'))
})

const WIN = 'C:\\Users\\me\\AppData\\Local\\Programs\\Kojoo Mail\\kojoomail-mcp.exe'

test('JSON 片段：Windows 路径的反斜杠转义后能被解析回原样', () => {
  const parsed = JSON.parse(mcpJson(WIN))
  assert.equal(parsed.mcpServers.kojoomail.command, WIN)
  assert.deepEqual(parsed.mcpServers.kojoomail.args, [])
})

test('TOML 片段：反斜杠和引号都转义', () => {
  const toml = mcpToml(WIN)
  assert.ok(toml.startsWith('[mcp_servers.kojoomail]\n'))
  assert.ok(toml.includes('C:\\\\Users\\\\me\\\\'))
  assert.ok(toml.endsWith('args = []\n'))
  assert.equal(mcpToml('a"b').includes('a\\"b'), true)
})

test('两种写法指向同一个可执行文件，没有客户端专属参数', () => {
  const posix = '/Applications/Kojoo Mail.app/Contents/MacOS/kojoomail-mcp'
  assert.equal(JSON.parse(mcpJson(posix)).mcpServers.kojoomail.command, posix)
  assert.ok(mcpToml(posix).includes(`command = "${posix}"`))
})

test('复制：clipboard 可用时走 clipboard', async () => {
  let got = ''
  setNav({ clipboard: { writeText: async (t) => { got = t } } })
  assert.equal(await copyText('hello'), true)
  assert.equal(got, 'hello')
})

test('复制：clipboard 抛异常时退回 execCommand，并且把临时 textarea 清掉', async () => {
  setNav({ clipboard: { writeText: async () => { throw new Error('denied') } } })
  let copied = ''
  document.execCommand = (cmd) => {
    copied = document.querySelector('textarea')?.value ?? ''
    return cmd === 'copy'
  }
  assert.equal(await copyText('fallback'), true)
  assert.equal(copied, 'fallback')
  assert.equal(document.querySelector('textarea'), null)
})

test('复制：两条路都失败时返回 false，不抛异常', async () => {
  setNav({})
  document.execCommand = () => false
  assert.equal(await copyText('nope'), false)
  assert.equal(document.querySelector('textarea'), null)
})