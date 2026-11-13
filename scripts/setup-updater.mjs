// 私钥和密码只写入忽略目录；CLI 输出包含私钥，必须捕获，不能转发到日志。
import { randomBytes } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'
import { ROOT, ENDPOINT, REPOSITORY, decodePublicKey, verifySignature } from './release.mjs'

const dir = join(ROOT, '.local/release-signing')
const key = join(dir, 'updater.key'), passwordPath = join(dir, 'password')
const configPath = join(ROOT, 'src-tauri/tauri.conf.json')
const config = JSON.parse(readFileSync(configPath, 'utf8'))
mkdirSync(dir, { recursive: true })
function quiet(command, args, options = {}) {
  const r = spawnSync(command, args, { cwd: ROOT, encoding: 'utf8', ...options })
  if (r.status !== 0) throw new Error(`${command} 操作失败；为避免输出密钥，已隐藏原始日志`)
}
if (!existsSync(key)) {
  if (config.plugins.updater.endpoints.includes(ENDPOINT)) throw new Error('fork 公钥已配置，请恢复原签名密钥备份；重新生成会中断已安装用户的更新')
  if (existsSync(passwordPath) || existsSync(key + '.pub')) throw new Error('检测到不完整密钥目录，请先检查备份，不自动覆盖')
  writeFileSync(passwordPath, randomBytes(32).toString('base64url'), { mode: 0o600, flag: 'wx' })
  quiet(process.execPath, ['node_modules/@tauri-apps/cli/tauri.js', 'signer', 'generate', '--ci', '--password', readFileSync(passwordPath, 'utf8'), '--write-keys', key])
}
const pubkey = readFileSync(key + '.pub', 'utf8').trim()
decodePublicKey(pubkey)
const env = { ...process.env, TAURI_SIGNING_PRIVATE_KEY: readFileSync(key, 'utf8').trim(), TAURI_SIGNING_PRIVATE_KEY_PASSWORD: readFileSync(passwordPath, 'utf8').trim() }
const probe = join(dir, 'probe.txt')
writeFileSync(probe, 'Kojoo Mail fork updater signing probe\n')
quiet(process.execPath, ['node_modules/@tauri-apps/cli/tauri.js', 'signer', 'sign', probe], { env })
verifySignature(readFileSync(probe), readFileSync(probe + '.sig', 'utf8').trim(), pubkey)
if (config.plugins.updater.endpoints.includes(ENDPOINT) && config.plugins.updater.pubkey !== pubkey) throw new Error('本地密钥与已提交的 fork 公钥不同，停止操作')
config.plugins.updater.pubkey = pubkey
config.plugins.updater.endpoints = [ENDPOINT]
writeFileSync(configPath, JSON.stringify(config, null, 2) + '\n')
if (process.argv.includes('--upload')) {
  for (const name of ['TAURI_SIGNING_PRIVATE_KEY', 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD']) {
    quiet('gh', ['secret', 'set', name, '--repo', REPOSITORY], { input: env[name] })
  }
  console.log('更新签名 Secret 已配置到 fork。')
}
console.log('独立更新公钥与地址已写入配置；私钥与密码保存在 .local/release-signing/，请离线备份。')
