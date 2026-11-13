import { test, before, after, afterEach } from 'node:test'
import assert from 'node:assert/strict'
import { JSDOM } from 'jsdom'
import React, { act } from 'react'
import { createRoot } from 'react-dom/client'
import { useMailPreview } from '../src/lib/use-mail-preview.ts'

let dom, host, root
const content = (body, signature = '') => ({ body, signature, quote: null })
const output = (preview) => ({ preview, html: `<div>${preview}</div>`, text: '纯文本' })

before(() => {
  dom = new JSDOM('<!doctype html><body></body>')
  globalThis.window = dom.window
  globalThis.document = dom.window.document
  globalThis.IS_REACT_ACT_ENVIRONMENT = true
})

afterEach(async () => {
  if (root) await act(async () => root.unmount())
  host?.remove()
  root = null
})

after(() => dom.window.close())

function mount(render) {
  host = document.createElement('div')
  document.body.append(host)
  root = createRoot(host)
  function Probe({ value }) {
    const preview = useMailPreview(value, render)
    return React.createElement('div', { 'aria-busy': preview.pending }, preview.error ? `错误：${preview.error.message}` : preview.html)
  }
  return async (value) => { await act(async () => root.render(React.createElement(Probe, { value }))) }
}

test('预览直接使用后端片段，不再次转换 Markdown 或转义实体', async () => {
  const calls = []
  const render = async (value) => { calls.push(value); return output('<p><strong>加粗</strong>&nbsp;</p>') }
  const update = mount(render)
  const value = { ...content('**加粗**', '<b>签名</b>'), quote: { header: '来源', text: '原文', html: '<p>原文</p>', reply: true } }
  await update(value)
  assert.deepEqual(calls, [value])
  assert.equal(host.textContent, '<p><strong>加粗</strong>&nbsp;</p>')
  assert.equal(host.firstChild.getAttribute('aria-busy'), 'false')
  await update({ ...value })
  assert.equal(calls.length, 1, '等价内容不应重复请求')
})

test('先发请求晚返回时，旧正文不会覆盖新正文或新账号签名', async () => {
  const requests = []
  const render = (value) => new Promise((resolve) => requests.push({ value, resolve }))
  const update = mount(render)
  await update(content('旧正文', '旧签名'))
  assert.equal(host.firstChild.getAttribute('aria-busy'), 'true')
  await update(content('新正文', '新签名'))
  await act(async () => requests[1].resolve(output('新正文与新签名')))
  assert.equal(host.textContent, '新正文与新签名')
  await act(async () => requests[0].resolve(output('过期结果')))
  assert.equal(host.textContent, '新正文与新签名')
})

test('内容变化时不短暂显示上一封邮件的预览，失败后可恢复', async () => {
  const requests = []
  const render = () => new Promise((resolve, reject) => requests.push({ resolve, reject }))
  const update = mount(render)
  await update(content('第一封'))
  await act(async () => requests[0].resolve(output('第一封预览')))
  await update(content('第二封'))
  assert.equal(host.textContent, '')
  assert.equal(host.firstChild.getAttribute('aria-busy'), 'true')
  await act(async () => requests[1].reject(new Error('渲染失败')))
  assert.equal(host.textContent, '错误：渲染失败')
  await update(content('第三封'))
  await act(async () => requests[2].resolve(output('第三封预览')))
  assert.equal(host.textContent, '第三封预览')
})

test('组件卸载后返回的渲染结果不会触发更新', async () => {
  let resolve
  const update = mount(() => new Promise((r) => { resolve = r }))
  await update(content('正文'))
  await act(async () => root.unmount())
  root = null
  await act(async () => resolve(output('已关闭的预览')))
  assert.equal(host.textContent, '')
})
