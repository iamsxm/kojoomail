// 与 Actions 共用 sidecar、签名、产物命名逻辑；可从 PowerShell 或 bash 调用。
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { ROOT, TARGETS, run } from './release.mjs'

const target = process.argv[2] || run('rustc', ['-vV']).match(/^host: (.+)$/m)?.[1]
if (!TARGETS[target]) throw new Error('仅支持 Windows x64、macOS arm64/x64；在对应操作系统原生构建')
const env = { ...process.env, CARGO_TARGET_DIR: join(ROOT, 'src-tauri/target') }
const oauth = join(ROOT, '.local/oauth.env')
if (existsSync(oauth)) {
  for (const line of readFileSync(oauth, 'utf8').split(/\r?\n/)) {
    const match = /^(KOJOOMAIL_(?:GOOGLE_CLIENT_ID|GOOGLE_CLIENT_SECRET|MS_CLIENT_ID))=(.*)$/.exec(line.trim())
    if (match && !env[match[1]]) env[match[1]] = match[2].replace(/^(['"])(.*)\1$/, '$2')
  }
}
const key = join(ROOT, '.local/release-signing/updater.key')
if (!env.TAURI_SIGNING_PRIVATE_KEY && existsSync(key)) {
  env.TAURI_SIGNING_PRIVATE_KEY = readFileSync(key, 'utf8').trim()
  env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD = readFileSync(join(ROOT, '.local/release-signing/password'), 'utf8').trim()
}
const mode = env.TAURI_SIGNING_PRIVATE_KEY ? 'signed' : 'preview'
const execute = (cmd, args) => run(cmd, args, { env, stdio: 'inherit' })
execute(process.execPath, ['scripts/release.mjs', 'check'])
execute(process.execPath, ['node_modules/typescript/bin/tsc', '-b'])
execute(process.execPath, ['node_modules/vite/bin/vite.js', 'build'])
execute('cargo', ['build', '--locked', '--release', '--manifest-path', 'src-tauri/Cargo.toml', '-p', 'kojoomail-mcp', '--target', target])
execute(process.execPath, ['scripts/desktop.mjs', 'stage', target, 'release'])
execute(process.execPath, ['scripts/desktop.mjs', 'bundle', target, 'release', mode])
execute(process.execPath, ['scripts/release.mjs', 'collect', target, mode])
