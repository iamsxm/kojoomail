import test from 'node:test'
import assert from 'node:assert/strict'
import { createHash, generateKeyPairSync, randomBytes, sign } from 'node:crypto'
import { cpSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { ROOT, TARGETS, ENDPOINT, parseVersion, checkVersions, setVersion, verifySignature, assetNames, collect, makeManifest } from '../scripts/release.mjs'

function fixture(t) {
  const dir = mkdtempSync(join(tmpdir(), 'kojoomail-release-'))
  t.after(() => rmSync(dir, { recursive: true, force: true }))
  return dir
}
function keys() {
  const pair = generateKeyPairSync('ed25519'), id = randomBytes(8)
  const raw = pair.publicKey.export({ format: 'der', type: 'spki' }).subarray(-32)
  const pub = Buffer.concat([Buffer.from('Ed'), id, raw])
  const pubkey = Buffer.from(`untrusted comment: test key\n${pub.toString('base64')}\n`).toString('base64')
  function signature(data, trusted = 'timestamp:1 file:test') {
    const sig = sign(null, createHash('blake2b512').update(data).digest(), pair.privateKey)
    const packet = Buffer.concat([Buffer.from('ED'), id, sig])
    const global = sign(null, Buffer.concat([sig, Buffer.from(trusted)]), pair.privateKey)
    return Buffer.from(`untrusted comment: signature\n${packet.toString('base64')}\ntrusted comment: ${trusted}\n${global.toString('base64')}\n`).toString('base64')
  }
  return { pubkey, signature }
}
function configFixture(t) {
  const root = fixture(t)
  for (const name of ['package.json', 'package-lock.json', 'src-tauri/tauri.conf.json', 'src-tauri/Cargo.toml', 'src-tauri/mcp-proxy/Cargo.toml', 'src-tauri/Cargo.lock']) {
    cpSync(join(ROOT, name), join(root, name), { recursive: true })
  }
  return root
}

test('release versions reject aliases, leading zeros and unsupported installer numbers', () => {
  for (const v of ['0.4.0', '1.2.3-rc.1', '0.4.0-beta.0']) assert.equal(parseVersion(v).version, v)
  for (const v of ['v0.4.0', '01.2.3', '1.2', '1.2.3+test', '1.2.3-rc.01', '65536.0.0', '1.2.3;echo bad']) assert.throws(() => parseVersion(v))
})

test('version command updates every app, bridge and lock version and detects mismatched tags', (t) => {
  const root = configFixture(t)
  setVersion('0.4.1-rc.2', root)
  assert.equal(checkVersions(root, 'v0.4.1-rc.2'), '0.4.1-rc.2')
  assert.throws(() => checkVersions(root, 'v0.4.0'), /标签/)
  const p = join(root, 'src-tauri/mcp-proxy/Cargo.toml')
  writeFileSync(p, readFileSync(p, 'utf8').replace('0.4.1-rc.2', '0.3.2'))
  assert.throws(() => checkVersions(root), /不一致/)
})

test('updater cannot silently return to upstream endpoint', (t) => {
  const root = configFixture(t), path = join(root, 'src-tauri/tauri.conf.json')
  const config = JSON.parse(readFileSync(path, 'utf8'))
  assert.equal(config.plugins.updater.endpoints[0], ENDPOINT)
  config.plugins.updater.endpoints = ['https://example.com/latest.json']
  writeFileSync(path, JSON.stringify(config))
  assert.throws(() => checkVersions(root), /更新源/)
})

test('updater verifies content, key and authenticated comment', () => {
  const key = keys(), wrong = keys(), data = Buffer.from('installer')
  const sig = key.signature(data)
  verifySignature(data, sig, key.pubkey)
  assert.throws(() => verifySignature(Buffer.from('tampered'), sig, key.pubkey), /验签失败/)
  assert.throws(() => verifySignature(data, sig, wrong.pubkey), /不匹配/)
  const text = Buffer.from(sig, 'base64').toString().replace('timestamp:1', 'timestamp:2')
  assert.throws(() => verifySignature(data, Buffer.from(text).toString('base64'), key.pubkey), /验签失败/)
})

function assetsFixture(t) {
  const dir = fixture(t), key = keys(), version = '0.4.0', commit = 'a'.repeat(40)
  for (const [target, spec] of Object.entries(TARGETS)) {
    const { files, update } = assetNames(version, target)
    const contents = { [update]: Buffer.from(target) }
    for (const name of files) {
      const data = name.endsWith('.sig') ? Buffer.from(key.signature(contents[update])) : contents[name] || Buffer.from('dmg')
      contents[name] = data
      writeFileSync(join(dir, name), data)
    }
    writeFileSync(join(dir, `record-${spec.platform}.json`), JSON.stringify({ version, commit, target, platform: spec.platform, signed: true,
      files: Object.fromEntries(files.map((name) => [name, createHash('sha256').update(contents[name]).digest('hex')])) }))
  }
  return { dir, key, version, commit }
}

test('manifest contains all three verified platforms and correctly versioned downloads', (t) => {
  const { dir, key, version, commit } = assetsFixture(t)
  const files = makeManifest(dir, version, commit, key.pubkey, 'release notes')
  const manifest = JSON.parse(readFileSync(join(dir, 'latest.json'), 'utf8'))
  assert.equal(manifest.version, version)
  assert.equal(manifest.notes, 'release notes')
  assert.deepEqual(Object.keys(manifest.platforms).sort(), ['darwin-aarch64', 'darwin-x86_64', 'windows-x86_64'])
  assert.match(manifest.platforms['windows-x86_64'].url, /iamsxm\/kojoomail\/releases\/download\/v0\.4\.0\/.*_setup\.exe$/)
  assert.ok(files.includes('SHA256SUMS.txt'))
})

test('publication rejects missing platform, changed content and wrong source commit', (t) => {
  const { dir, key, version, commit } = assetsFixture(t)
  assert.throws(() => makeManifest(dir, version, 'b'.repeat(40), key.pubkey, ''), /提交/)
  const file = assetNames(version, 'aarch64-apple-darwin').update
  writeFileSync(join(dir, file), 'tampered')
  assert.throws(() => makeManifest(dir, version, commit, key.pubkey, ''), /摘要/)
  rmSync(join(dir, 'record-darwin-aarch64.json'))
  assert.throws(() => makeManifest(dir, version, commit, key.pubkey, ''), /三个平台/)
})

test('collector renames Windows installer without changing its signature', (t) => {
  const root = configFixture(t), key = keys(), configPath = join(root, 'src-tauri/tauri.conf.json')
  const config = JSON.parse(readFileSync(configPath, 'utf8'))
  config.plugins.updater.pubkey = key.pubkey
  writeFileSync(configPath, JSON.stringify(config))
  const target = 'x86_64-pc-windows-msvc'
  const dir = join(root, 'src-tauri/target', target, 'release/bundle/nsis')
  mkdirSync(dir, { recursive: true })
  const data = Buffer.from('installer data'), sig = key.signature(data)
  writeFileSync(join(dir, 'Kojoo Mail_setup.exe'), data)
  writeFileSync(join(dir, 'Kojoo Mail_setup.exe.sig'), sig)
  const out = collect(target, true, root, 'a'.repeat(40))
  const name = assetNames(config.version, target).update
  assert.deepEqual(readFileSync(join(out, name)), data)
  verifySignature(data, readFileSync(join(out, name + '.sig'), 'utf8').trim(), key.pubkey)
})
