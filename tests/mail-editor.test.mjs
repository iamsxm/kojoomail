import { test, before, after } from 'node:test'
import assert from 'node:assert/strict'
import { JSDOM } from 'jsdom'
import { Editor } from '@tiptap/core'
import { mailEditorExtensions, MAIL_FONTS, selectionTextStyle } from '../src/lib/mail-editor.ts'
import { needsSourceMode } from '../src/lib/mail-source.ts'
import React, { act } from 'react'
import { createRoot } from 'react-dom/client'
import { useEditorPopovers } from '../src/lib/use-editor-popovers.ts'

let dom
before(() => {
  dom = new JSDOM('<!doctype html><html><body></body></html>', { pretendToBeVisual: true })
  for (const key of ['window', 'document', 'Node', 'HTMLElement', 'Element', 'MutationObserver', 'DOMParser', 'getComputedStyle', 'requestAnimationFrame', 'cancelAnimationFrame']) {
    globalThis[key] = typeof dom.window[key] === 'function' && ['getComputedStyle', 'requestAnimationFrame', 'cancelAnimationFrame'].includes(key) ? dom.window[key].bind(dom.window) : dom.window[key]
  }
  Object.defineProperty(globalThis, 'navigator', { configurable: true, value: dom.window.navigator })
  globalThis.IS_REACT_ACT_ENVIRONMENT = true
})
after(() => dom.window.close())

function withEditor(content, run, options = {}) {
  const editor = new Editor({ extensions: mailEditorExtensions(), content, contentType: 'markdown', ...options })
  try { run(editor) } finally { editor.destroy() }
}

test('打开原有 Markdown 不触发改写，五列表格、中文、转义符与软换行可视化读取', () => {
  const source = '您好！\n请审阅：\n\n| 项目 | 本周（11-09 \\~ 11-13） | 下周 | 里程碑 | 风险 |\n|:---|:---:|---:|---|---|\n| 平台 | ① 完成 **优化**；② 测试 | 持续跟进 | 已上线 | 无 |\n\n以上，请审阅。'
  let updates = 0
  withEditor(source, (e) => {
    const table = e.getJSON().content.find((n) => n.type === 'table')
    assert.equal(table.content[0].content.length, 5)
    assert.equal(table.content[1].content.length, 5)
    assert.match(e.getText(), /11-09 ~ 11-13/)
    assert.match(e.getHTML(), /<strong>优化<\/strong>/)
    assert.match(e.getHTML(), /<br>/)
    assert.equal(updates, 0, '初次解析不能触发自动保存覆盖原 Markdown')
    withEditor(e.getMarkdown(), (restored) => {
      assert.equal(restored.getText(), e.getText())
      assert.equal(restored.getJSON().content.find((n) => n.type === 'table').content[0].content.length, 5)
    })
  }, { onUpdate: () => updates++ })
})

test('表格加行加列、列宽、单元格多段落在序列化及重开后保留', () => {
  withEditor('| A | B |\n|---|---|\n| 一 | 二 |', (e) => {
    e.commands.setTextSelection(3)
    assert.equal(e.commands.addRowAfter(), true)
    assert.equal(e.commands.addColumnAfter(), true)
    assert.equal(e.commands.setCellAttribute('colwidth', [220]), true)
    const stored = e.getMarkdown()
    assert.match(stored, /<table/)
    assert.match(stored, /border-collapse/)
    assert.match(stored, /colwidth="220"/)
    assert.equal(needsSourceMode(stored), false, stored)
    assert.equal(needsSourceMode(stored.replace('padding: 8px 10px', 'font-size: 30px; padding: 8px 10px')), true, '手动添加的自定义样式不能被标记掩盖')
    withEditor(stored, (restored) => {
      const table = restored.getJSON().content.find((n) => n.type === 'table')
      assert.equal(table.content.length, 3)
      assert.equal(table.content[0].content.length, 3)
      assert.deepEqual(table.content[0].content[0].attrs.colwidth, [220])
    })
  })
  const source = '<table><tr><th>A</th></tr><tr><td><p>第一段</p><p>第二段</p></td></tr></table>'
  withEditor(source, (e) => withEditor(e.getMarkdown(), (restored) => {
    assert.match(restored.getText(), /第一段/)
    assert.match(restored.getText(), /第二段/)
  }))
})

test('自定义 HTML 样式进入源码模式，普通 Markdown 和编辑器下划线可直接编辑', () => {
  assert.equal(needsSourceMode('<span style="color:red">正文</span>'), true)
  assert.equal(needsSourceMode('<table border="0"><tr><td>布局</td></tr></table>'), true)
  assert.equal(needsSourceMode('**粗体** <u>下划线</u>\n\n正文'), false)
})

test('粗体、下划线、列表、任务、图片和安全链接编辑后可重开；撤销恢复正文', () => {
  withEditor('**加粗** <u>强调</u>\n\n- 列表\n\n- [x] 已完成\n\n[链接](https://example.com)\n\n![图片](https://example.com/p.png)', (e) => {
    const previous = e.getMarkdown()
    e.commands.insertContent('修改')
    assert.match(e.getText(), /修改/)
    e.commands.undo()
    assert.equal(e.getMarkdown().trimEnd(), previous.trimEnd())
    withEditor(e.getMarkdown(), (restored) => {
      const html = restored.getHTML()
      for (const expected of ['<strong>加粗</strong>', '<u>强调</u>', 'taskList', 'https://example.com/p.png', 'href="https://example.com"']) assert.ok(html.includes(expected), html)
    })
  })
})

test('外部 Markdown（AI 或源码修改）同步不会再次触发 onUpdate', () => {
  let updates = 0
  withEditor('旧正文', (e) => {
    e.commands.setContent('新的 **正文**', { contentType: 'markdown', emitUpdate: false })
    assert.match(e.getHTML(), /<strong>正文<\/strong>/)
    assert.equal(updates, 0)
  }, { onUpdate: () => updates++ })
})

test('字体、字号、文字和背景颜色与粗斜体链接代码组合，保存 Markdown 后保留文字格式', () => {
  withEditor('**加粗** *斜体* <u>下划线</u> ~~删除~~ [链接](https://example.com) `x < y`\n下一行', (e) => {
    e.commands.selectAll()
    e.chain().setFontFamily(MAIL_FONTS[0][2]).setFontSize('24px').setColor('#2563eb').setBackgroundColor('#fef08a').run()
    const stored = e.getMarkdown()
    assert.match(stored, /data-kojoomail-style="1"/)
    assert.match(stored, /font-size: 24px/)
    assert.equal(needsSourceMode(stored), false, stored)
    const textFormats = (editor) => {
      const parts = []
      editor.state.doc.descendants((node) => { if (node.isText && node.text.trim()) parts.push({ text: node.text.trim(), marks: node.marks.map((mark) => ({ type: mark.type.name, attrs: { ...mark.attrs } })) }) })
      return JSON.parse(JSON.stringify(parts).replace(/#[0-9a-f]{6}/g, (hex) => `rgb(${[1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)).join(', ')})`))
    }
    withEditor(stored, (restored) => {
      assert.deepEqual(textFormats(restored), textFormats(e), stored)
      assert.equal(restored.getText(), e.getText())
    })
  })
})

test('中英文预设字体和字号均可序列化，普通正文依然保存成 Markdown', () => {
  for (const [, , family] of MAIL_FONTS) withEditor('字体样例', (e) => {
    e.commands.selectAll()
    e.chain().setFontFamily(family).setFontSize('96px').run()
    const stored = e.getMarkdown()
    assert.equal(needsSourceMode(stored), false, stored)
    withEditor(stored, (restored) => assert.equal(restored.getHTML(), e.getHTML(), family))
  })
  withEditor('**普通粗体**', (e) => assert.equal(e.getMarkdown().trim(), '**普通粗体**'))
})

test('有颜色的表格继续支持 GFM 和复杂 HTML 表格两种保存方式', () => {
  withEditor('| A | B |\n|---|---|\n| **优化** | 二 |', (e) => {
    let from
    e.state.doc.descendants((node, pos) => { if (node.isText && node.text === '优化') from = pos })
    assert.equal(typeof from, 'number')
    e.commands.setTextSelection({ from, to: from + 2 })
    e.chain().setColor('#dc2626').setFontFamily(MAIL_FONTS[1][2]).setFontSize('18px').run()
    const simple = e.getMarkdown()
    assert.ok(simple.trimStart().startsWith('|'), simple)
    assert.equal(needsSourceMode(simple), false)
    withEditor(simple, (restored) => assert.equal(restored.getHTML(), e.getHTML(), simple))
    e.commands.setCellAttribute('colwidth', [220])
    const complex = e.getMarkdown()
    assert.match(complex, /<table/)
    assert.equal(needsSourceMode(complex), false, complex)
    withEditor(complex, (restored) => assert.equal(restored.getHTML(), e.getHTML(), complex))
  })
})

test('重置一种文字样式不影响其他样式，清除格式后恢复纯 Markdown', () => {
  withEditor('文字', (e) => {
    e.commands.selectAll()
    e.chain().setFontFamily('Arial, Helvetica, sans-serif').setFontSize('20px').setColor('#2563eb').setBackgroundColor('#fef08a').run()
    e.commands.unsetColor()
    assert.equal(e.getAttributes('textStyle').color, null)
    assert.equal(e.getAttributes('textStyle').fontSize, '20px')
    e.commands.unsetFontFamily()
    assert.equal(e.getAttributes('textStyle').fontSize, '20px')
    e.commands.unsetBackgroundColor()
    assert.equal(e.getAttributes('textStyle').backgroundColor, null)
    e.commands.unsetFontSize()
    assert.equal(e.getMarkdown().trim(), '文字')
    e.commands.setColor('#dc2626')
    const colored = e.getMarkdown()
    e.commands.undo()
    e.commands.redo()
    assert.equal(e.getMarkdown(), colored)
    e.commands.unsetAllMarks()
    assert.equal(e.getMarkdown().trim(), '文字')
  })
})

test('输入点继承字体颜色，混合选区显示混合值，避免误报第一个片段的格式', () => {
  withEditor('第一段 第二段', (e) => {
    e.commands.setTextSelection({ from: 1, to: 4 })
    e.chain().setFontSize('24px').setColor('#2563eb').run()
    e.commands.selectAll()
    assert.equal(selectionTextStyle(e).fontSize, 'mixed')
    assert.equal(selectionTextStyle(e).color, 'mixed')
    assert.equal(selectionTextStyle(e).fontFamily, '')
    e.commands.setTextSelection(e.state.doc.content.size - 1)
    e.chain().setFontSize('18px').setColor('#dc2626').insertContent('新输入').run()
    assert.equal(selectionTextStyle(e).fontSize, '18px')
    withEditor(e.getMarkdown(), (restored) => assert.equal(restored.getHTML(), e.getHTML()))
  })
})

test('编辑器标记不能掩盖未支持的手写样式、事件或嵌入 HTML', () => {
  const style = '<span data-kojoomail-style="1" style="font-size: 16px; color: rgb(37, 99, 235)">文字</span>'
  assert.equal(needsSourceMode(style), false)
  for (const custom of [
    style.replace('font-size:', 'line-height: 1.8; font-size:'),
    style.replace('font-size:', '--custom: red; font-size:'),
    style.replace('16px', '200px'), style.replace('16px', '16px !important'),
    style.replace('rgb(37, 99, 235)', 'var(--custom-color)'),
    style.replace('文字', '<script>alert(1)</script>'), style.replace('style=', 'onclick="alert(1)" style='),
    style.replace('文字', '<em style="letter-spacing: 2px">文字</em>'),
    style.replace('文字', '<strong onclick="alert(1)">文字</strong>'),
  ]) assert.equal(needsSourceMode(custom), true, custom)
})

test('跨越粗体斜体边界的不同颜色和字号，切换 Markdown 后不改变文字格式', () => {
  withEditor('abcdefghijkl', (e) => {
    e.commands.setTextSelection({ from: 1, to: 8 }); e.commands.setBold()
    e.commands.setTextSelection({ from: 4, to: 11 }); e.commands.setItalic()
    e.commands.setTextSelection({ from: 3, to: 6 }); e.chain().setColor('#2563eb').setFontSize('24px').run()
    e.commands.setTextSelection({ from: 6, to: 10 }); e.chain().setColor('#dc2626').setFontSize('18px').run()
    const stored = e.getMarkdown()
    assert.equal(needsSourceMode(stored), false, stored)
    withEditor(stored, (restored) => assert.equal(restored.getHTML(), e.getHTML(), stored))
  })
})

test('正文保留焦点时 Esc 先关闭格式菜单；切换模式清理隐藏菜单和监听器', async () => {
  const host = document.createElement('div')
  document.body.append(host)
  const root = createRoot(host)
  let controls, closes = 0
  const sheetEscape = (e) => { if (e.key === 'Escape') closes++ }
  function Probe({ active }) {
    controls = useEditorPopovers(active)
    return React.createElement('div', { contentEditable: true, tabIndex: 0, role: 'textbox', 'aria-label': '正文' })
  }
  window.addEventListener('keydown', sheetEscape)
  try {
    await act(async () => root.render(React.createElement(Probe, { active: true })))
    await act(async () => controls.setTableOpen(true))
    host.firstChild.focus()
    await act(async () => host.firstChild.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })))
    assert.equal(controls.tableOpen, false)
    assert.equal(closes, 0, '正文焦点不应导致整封草稿关闭')
    await act(async () => controls.setColorOpen('text'))
    await act(async () => host.firstChild.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })))
    assert.equal(controls.colorOpen, null)
    assert.equal(closes, 0, '颜色菜单的 Esc 不能关闭草稿')
    await act(async () => controls.setColorOpen('background'))
    await act(async () => controls.setLinkOpen(true))
    await act(async () => root.render(React.createElement(Probe, { active: false })))
    assert.equal(controls.linkOpen, false)
    assert.equal(controls.colorOpen, null)
    await act(async () => host.firstChild.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: 'Escape', bubbles: true })))
    assert.equal(closes, 1, '隐藏菜单不能继续拦截其他编辑模式的 Esc')
  } finally {
    window.removeEventListener('keydown', sheetEscape)
    await act(async () => root.unmount())
    host.remove()
  }
})
