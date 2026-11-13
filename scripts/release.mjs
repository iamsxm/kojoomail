// 发布工具仅使用 Node 标准库，CI 与本地使用同一套版本和产物校验。
import { createHash, createPublicKey, verify } from 'node:crypto'
import { copyFileSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs'
import { resolve, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { execFileSync, spawnSync } from 'node:child_process'

export const ROOT = fileURLToPath(new URL('../', import.meta.url))
export const REPOSITORY = 'iamsxm/kojoomail'
export const ENDPOINT = `https://github.com/${REPOSITORY}/releases/latest/download/latest.json`
export const TARGETS = {
  'x86_64-pc-windows-msvc': { platform: 'windows-x86_64', label: 'windows_x86_64', bundles: 'nsis' },
  'aarch64-apple-darwin': { platform: 'darwin-aarch64', label: 'macos_aarch64', bundles: 'app,dmg' },
  'x86_64-apple-darwin': { platform: 'darwin-x86_64', label: 'macos_x86_64', bundles: 'app,dmg' },
}

const json = (path) => JSON.parse(readFileSync(path, 'utf8'))
const saveJson = (path, value) => writeFileSync(path, JSON.stringify(value, null, 2) + '\n')
const sha256 = (path) => createHash('sha256').update(readFileSync(path)).digest('hex')
export function run(command, args, options = {}) {
  return (execFileSync(command, args, { cwd: ROOT, encoding: 'utf8', ...options }) ?? '').trim()
}
export function appleBuildEnv(source) {
  const env = { ...source }
  // 空 Secret 不等于未配置：Tauri 看到变量存在就会尝试导入证书。
  for (const name of ['APPLE_CERTIFICATE', 'APPLE_CERTIFICATE_PASSWORD', 'APPLE_ID', 'APPLE_PASSWORD', 'APPLE_TEAM_ID', 'APPLE_SIGNING_IDENTITY']) {
    if (!env[name]) delete env[name]
  }
  return env
}
export function parseVersion(version) {
  // 限定发布格式，避免 NSIS 版本资源、标签和 SemVer 的解释分歧。
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-(alpha|beta|rc)\.(0|[1-9]\d*))?$/.exec(version)
  if (!match || match.slice(1, 4).some((n) => Number(n) > 65535)) throw new Error(`无效发布版本：${version}`)
  return { version, prerelease: Boolean(match[4]), parts: match.slice(1, 4).map(Number) }
}
function cargoPackageVersion(text, name) {
  const blocks = text.split(/(?=^\[\[package\]\])/m)
  const block = blocks.find((b) => b.includes(`name = "${name}"`))
  const version = block?.match(/^version = "([^"]+)"/m)?.[1]
  if (!version) throw new Error(`Cargo.lock 缺少 ${name} 的版本`)
  return version
}
export function checkVersions(root = ROOT, tag) {
  const pkg = json(join(root, 'package.json'))
  parseVersion(pkg.version)
  const lock = json(join(root, 'package-lock.json'))
  const config = json(join(root, 'src-tauri/tauri.conf.json'))
  const cargoLock = readFileSync(join(root, 'src-tauri/Cargo.lock'), 'utf8')
  const values = {
    'package-lock.json': lock.version, 'package-lock root': lock.packages[''].version,
    'tauri.conf.json': config.version,
    'Cargo.lock app': cargoPackageVersion(cargoLock, 'kojoomail'),
    'Cargo.lock bridge': cargoPackageVersion(cargoLock, 'kojoomail-mcp'),
  }
  for (const path of ['src-tauri/Cargo.toml', 'src-tauri/mcp-proxy/Cargo.toml']) {
    values[path] = readFileSync(join(root, path), 'utf8').match(/^version = "([^"]+)"/m)?.[1]
  }
  for (const [path, value] of Object.entries(values)) {
    if (value !== pkg.version) throw new Error(`${path} 版本 ${value} 与 package.json ${pkg.version} 不一致`)
  }
  if (tag && tag !== `v${pkg.version}`) throw new Error(`标签 ${tag} 与源码版本 v${pkg.version} 不一致`)
  if (JSON.stringify(config.plugins.updater.endpoints) !== JSON.stringify([ENDPOINT])) {
    throw new Error('updater 必须使用 fork 的 Release 更新源')
  }
  decodePublicKey(config.plugins.updater.pubkey)
  return pkg.version
}
export function setVersion(version, root = ROOT) {
  parseVersion(version)
  for (const path of ['package.json', 'package-lock.json', 'src-tauri/tauri.conf.json']) {
    const value = json(join(root, path))
    value.version = version
    if (path === 'package-lock.json') value.packages[''].version = version
    saveJson(join(root, path), value)
  }
  for (const path of ['src-tauri/Cargo.toml', 'src-tauri/mcp-proxy/Cargo.toml']) {
    const file = join(root, path)
    const value = readFileSync(file, 'utf8')
    if (!/^version = "[^"]+"/m.test(value)) throw new Error(`缺少版本：${path}`)
    writeFileSync(file, value.replace(/^version = "[^"]+"/m, `version = "${version}"`))
  }
  const file = join(root, 'src-tauri/Cargo.lock')
  writeFileSync(file, readFileSync(file, 'utf8').replace(
    /(^name = "(?:kojoomail|kojoomail-mcp)"\r?\nversion = ")[^"]+("$)/gm, `$1${version}$2`,
  ))
  checkVersions(root)
}
function base64(value) {
  const s = value.trim()
  if (!s || !/^[A-Za-z0-9+/]+={0,2}$/.test(s) || s.length % 4 !== 0) throw new Error('无效的 base64 签名或公钥')
  return Buffer.from(s, 'base64')
}
export function decodePublicKey(encoded) {
  const lines = base64(encoded).toString('utf8').trim().split(/\r?\n/)
  if (lines.length !== 2 || !lines[0].startsWith('untrusted comment: ')) throw new Error('无效的 Tauri 公钥格式')
  const packet = base64(lines[1])
  if (packet.length !== 42 || packet.subarray(0, 2).toString() !== 'Ed') throw new Error('无效的 Minisign 公钥')
  return packet
}
// 与 tauri-plugin-updater 的 Minisign 校验保持一致：数据签名 + trusted comment 签名。
export function verifySignature(data, encodedSignature, encodedPublicKey) {
  const pub = decodePublicKey(encodedPublicKey)
  const lines = base64(encodedSignature).toString('utf8').trim().split(/\r?\n/)
  if (lines.length !== 4 || !lines[0].startsWith('untrusted comment: ') || !lines[2].startsWith('trusted comment: ')) {
    throw new Error('无效的 Tauri 签名格式')
  }
  const packet = base64(lines[1]), global = base64(lines[3])
  const algo = packet.subarray(0, 2).toString()
  if (packet.length !== 74 || global.length !== 64 || !['Ed', 'ED'].includes(algo) || !packet.subarray(2, 10).equals(pub.subarray(2, 10))) {
    throw new Error('签名与更新公钥不匹配')
  }
  const key = createPublicKey({ key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), pub.subarray(10)]), format: 'der', type: 'spki' })
  const sig = packet.subarray(10)
  const message = algo === 'ED' ? createHash('blake2b512').update(data).digest() : data
  if (!verify(null, message, key, sig) || !verify(null, Buffer.concat([sig, Buffer.from(lines[2].slice(17))]), key, global)) {
    throw new Error('更新产物验签失败')
  }
}
export function assetNames(version, target, signed = true) {
  const spec = TARGETS[target]
  if (!spec) throw new Error(`不支持的目标：${target}`)
  const stem = `KojooMail_${version}_${spec.label}`
  const update = spec.platform.startsWith('windows') ? `${stem}_setup.exe` : `${stem}.app.tar.gz`
  const files = spec.platform.startsWith('windows') ? [update] : [`${stem}.dmg`, ...(signed ? [update] : [])]
  if (signed) files.push(`${update}.sig`)
  return { update, files }
}
function findOne(dir, suffix) {
  const files = readdirSync(dir).filter((name) => name.endsWith(suffix))
  if (files.length !== 1) throw new Error(`${dir} 应当恰有一个 ${suffix}，实际 ${files.length}`)
  return join(dir, files[0])
}
export function collect(target, signed = true, root = ROOT, commit = run('git', ['rev-parse', 'HEAD'])) {
  const version = checkVersions(root), spec = TARGETS[target]
  if (!spec) throw new Error(`不支持的目标：${target}`)
  const names = assetNames(version, target, signed)
  const bundle = join(root, 'src-tauri/target', target, 'release/bundle')
  const out = join(root, 'release-assets', target)
  mkdirSync(out, { recursive: true })
  const pairs = []
  if (spec.platform.startsWith('windows')) {
    pairs.push([findOne(join(bundle, 'nsis'), '.exe'), names.update])
  } else {
    pairs.push([findOne(join(bundle, 'dmg'), '.dmg'), names.files[0]])
    if (signed) pairs.push([findOne(join(bundle, 'macos'), '.app.tar.gz'), names.update])
  }
  for (const [source, name] of pairs) {
    copyFileSync(source, join(out, name))
    if (signed && name === names.update) {
      const signature = readFileSync(source + '.sig', 'utf8').trim()
      verifySignature(readFileSync(source), signature, json(join(root, 'src-tauri/tauri.conf.json')).plugins.updater.pubkey)
      writeFileSync(join(out, name + '.sig'), signature + '\n')
    }
  }
  const record = { version, target, platform: spec.platform, commit, signed, files: Object.fromEntries(names.files.map((name) => [name, sha256(join(out, name))])) }
  saveJson(join(out, `record-${spec.platform}.json`), record)
  return out
}
export function makeManifest(dir, version, commit, publicKey, notes, date = new Date().toISOString()) {
  parseVersion(version)
  const platforms = {}, files = []
  const records = readdirSync(dir).filter((name) => /^record-.*\.json$/.test(name))
  if (records.length !== 3) throw new Error('必须具备三个平台的构建记录')
  for (const [target, spec] of Object.entries(TARGETS)) {
    const record = json(join(dir, `record-${spec.platform}.json`))
    const names = assetNames(version, target)
    if (record.version !== version || record.commit !== commit || record.target !== target || record.platform !== spec.platform || record.signed !== true) {
      throw new Error(`${spec.platform} 构建版本、提交或签名状态不一致`)
    }
    if (JSON.stringify(Object.keys(record.files).sort()) !== JSON.stringify([...names.files].sort())) throw new Error(`${spec.platform} 文件集合不完整`)
    for (const name of names.files) {
      if (sha256(join(dir, name)) !== record.files[name]) throw new Error(`产物摘要不符：${name}`)
      files.push(name)
    }
    const signature = readFileSync(join(dir, names.update + '.sig'), 'utf8').trim()
    verifySignature(readFileSync(join(dir, names.update)), signature, publicKey)
    platforms[spec.platform] = { signature, url: `https://github.com/${REPOSITORY}/releases/download/v${version}/${names.update}` }
  }
  saveJson(join(dir, 'latest.json'), { version, notes, pub_date: date, platforms })
  files.push('latest.json')
  writeFileSync(join(dir, 'SHA256SUMS.txt'), files.sort().map((name) => `${sha256(join(dir, name))}  ${name}\n`).join(''))
  files.push('SHA256SUMS.txt')
  return files
}
function releaseByTag(tag) {
  const result = spawnSync('gh', ['api', `repos/${REPOSITORY}/releases/tags/${tag}`], { encoding: 'utf8' })
  if (result.status === 0) return JSON.parse(result.stdout)
  if (result.stderr?.includes('HTTP 404')) return null
  throw new Error(`无法检查 Release：${result.stderr || result.error?.message}`)
}
export function validateRelease(tag) {
  const version = checkVersions(ROOT, tag)
  if (process.env.GITHUB_REPOSITORY && process.env.GITHUB_REPOSITORY !== REPOSITORY) throw new Error('正式发布仅在维护者 fork 中启用')
  const commit = run('git', ['rev-parse', 'HEAD'])
  if (run('git', ['rev-parse', `${tag}^{commit}`]) !== commit) throw new Error('检出提交与标签不一致')
  run('git', ['merge-base', '--is-ancestor', commit, 'origin/main'])
  const existing = releaseByTag(tag)
  if (existing && !existing.draft) throw new Error('该标签已公开发布；修复请使用新版本，不覆盖用户已安装的版本')
  const latest = spawnSync('gh', ['api', `repos/${REPOSITORY}/releases/latest`], { encoding: 'utf8' })
  if (!parseVersion(version).prerelease && latest.status === 0) {
    const old = parseVersion(JSON.parse(latest.stdout).tag_name.replace(/^v/, ''))
    const next = parseVersion(version)
    const diff = next.parts.map((n, i) => n - old.parts[i]).find((n) => n !== 0) ?? 0
    if (diff <= 0) throw new Error('稳定版版本号必须高于当前 latest，避免更新入口倒退')
  } else if (latest.status !== 0 && !latest.stderr?.includes('HTTP 404')) {
    throw new Error(`无法检查 latest Release：${latest.stderr || latest.error?.message}`)
  }
  if (process.env.GITHUB_OUTPUT) writeFileSync(process.env.GITHUB_OUTPUT, `version=${version}\ncommit=${commit}\ntag=${tag}\n`, { flag: 'a' })
  return { version, commit }
}
function publish(tag) {
  const { version, commit } = validateRelease(tag)
  const dir = join(ROOT, 'release-assets')
  const notes = run('gh', ['api', `repos/${REPOSITORY}/releases/generate-notes`, '-f', `tag_name=${tag}`, '--jq', '.body']) +
    `\n\nSource: ${commit}\n\nmacOS: builds use ad-hoc signing unless Apple signing credentials are configured. On first launch you may need to allow the app in System Settings > Privacy & Security.\n`
  const notesFile = join(dir, 'release-notes.md')
  writeFileSync(notesFile, notes + '\n')
  const files = makeManifest(dir, version, commit, json(join(ROOT, 'src-tauri/tauri.conf.json')).plugins.updater.pubkey, notes)
  if (!releaseByTag(tag)) run('gh', ['release', 'create', tag, '--repo', REPOSITORY, '--verify-tag', '--draft', '--title', `Kojoo Mail ${tag}`, '--notes-file', notesFile])
  run('gh', ['release', 'upload', tag, '--repo', REPOSITORY, '--clobber', ...files.map((name) => join(dir, name))])
  // 所有构建、测试、验签和上传均已成功，此处才公开。
  const pre = parseVersion(version).prerelease
  run('gh', ['release', 'edit', tag, '--repo', REPOSITORY, '--draft=false', `--prerelease=${pre}`, `--latest=${!pre}`, '--notes-file', notesFile])
  console.log(`已公开 Release：${tag}`)
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [command, arg, extra] = process.argv.slice(2)
    if (command === 'check') console.log(`版本和更新配置一致：${checkVersions(ROOT, arg)}`)
    else if (command === 'version') { setVersion(arg); console.log(`版本已统一为 ${arg}`) }
    else if (command === 'validate') validateRelease(arg)
    else if (command === 'collect') console.log(collect(arg, extra === 'signed'))
    else if (command === 'publish') publish(arg)
    else throw new Error('用法：release.mjs check [v标签] | version 版本 | validate v标签 | collect target signed/preview | publish v标签')
  } catch (error) { console.error(error.message); process.exitCode = 1 }
}
