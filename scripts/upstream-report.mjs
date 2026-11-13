import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { ROOT, run } from './release.mjs'

const requested = process.argv[2] || 'origin/main'
const upstream = run('git', ['rev-parse', '--verify', `${requested}^{commit}`])
const current = run('git', ['rev-parse', 'HEAD'])
const base = run('git', ['merge-base', current, upstream])
const files = run('git', ['diff', '--name-only', `${base}..${upstream}`]).split('\n').filter(Boolean)
const commits = run('git', ['log', '--format=%h %s', `${current}..${upstream}`])
const watch = /(^src-tauri\/(src\/(mcp.*|commands|settings|store|lib|assist|mailbox|compose|html|smtp)\.rs|mcp-proxy\/)|^src\/(store\.ts|main\.tsx|ui\.tsx|views\/(Settings|Compose|UpdateDialog)\.tsx)|^src-tauri\/(Cargo\.(toml|lock)|tauri\.conf\.json)|^\.github\/)/
const tracked = JSON.parse(readFileSync(join(ROOT, 'docs/upstream-state.json'), 'utf8'))
const report = `# 上游差异报告\n\n生成时间：${new Date().toISOString()}\n\n` +
  `- 当前 fork：\`${current}\`\n- 官方目标：\`${upstream}\`\n- 共同祖先：\`${base}\`\n- 已记录合入的官方提交：\`${tracked.integratedCommit}\`\n\n` +
  `## 尚未合入的官方提交\n\n${commits ? '```text\n' + commits + '\n```' : '无。官方目标已在当前分支历史内。'}\n\n` +
  `## 官方修改的文件\n\n${files.map((f) => '- `' + f + '`').join('\n') || '无。'}\n\n` +
  `## 必须人工审阅的共享文件\n\n${files.filter((f) => watch.test(f)).map((f) => '- `' + f + '`').join('\n') || '本次未命中重点文件。'}\n\n` +
  `报告不执行合并。保留 fork MCP，数据库迁移只能追加；共享文件逐块合并。按 docs/github-workflow.zh-CN.md 创建同步分支并保留 merge ancestry。\n`
mkdirSync(join(ROOT, 'release-assets'), { recursive: true })
writeFileSync(join(ROOT, 'release-assets/upstream-report.md'), report)
if (process.env.GITHUB_STEP_SUMMARY) writeFileSync(process.env.GITHUB_STEP_SUMMARY, report, { flag: 'a' })
console.log(report)
