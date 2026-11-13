import { copyFileSync, chmodSync, mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { ROOT, TARGETS, run } from './release.mjs'

const [command, target, profile = 'release', mode = 'preview'] = process.argv.slice(2)
if (!TARGETS[target] || !['debug', 'release'].includes(profile)) throw new Error('需要受支持的 target 和 debug/release profile')
const windows = target.includes('windows')
const filename = `kojoomail-mcp${windows ? '.exe' : ''}`
if (command === 'stage') {
  const dir = join(ROOT, 'src-tauri/binaries')
  mkdirSync(dir, { recursive: true })
  const dest = join(dir, `kojoomail-mcp-${target}${windows ? '.exe' : ''}`)
  copyFileSync(join(ROOT, 'src-tauri/target', target, profile, filename), dest)
  if (!windows) chmodSync(dest, 0o755)
  console.log(`已准备 ${target} MCP 代理`)
} else if (command === 'bundle') {
  if (!['signed', 'preview'].includes(mode) || profile !== 'release') throw new Error('打包需要 release 和 signed/preview')
  const signed = mode === 'signed'
  if (signed && !process.env.TAURI_SIGNING_PRIVATE_KEY) throw new Error('缺少更新签名私钥，停止正式打包')
  const config = { bundle: { createUpdaterArtifacts: signed } }
  if (!windows && process.env.APPLE_SIGNING_IDENTITY) config.bundle.macOS = { signingIdentity: process.env.APPLE_SIGNING_IDENTITY }
  mkdirSync(join(ROOT, '.local'), { recursive: true })
  const path = join(ROOT, '.local/ci-bundle.json')
  writeFileSync(path, JSON.stringify(config))
  run(process.execPath, ['node_modules/@tauri-apps/cli/tauri.js', 'build', '--ci', '--target', target,
    '--bundles', TARGETS[target].bundles, '--config', path, '--', '--locked'], { stdio: 'inherit' })
} else throw new Error('用法：desktop.mjs stage|bundle target debug|release [signed|preview]')
