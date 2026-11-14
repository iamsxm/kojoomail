import { generateHTML, type Editor, type JSONContent } from '@tiptap/core'
import StarterKit from '@tiptap/starter-kit'
import { Markdown } from '@tiptap/markdown'
import { Table, TableCell, TableHeader, TableRow } from '@tiptap/extension-table'
import Image from '@tiptap/extension-image'
import { TaskItem, TaskList } from '@tiptap/extension-list'
import { TextStyle, FontFamily, FontSize, Color, BackgroundColor } from '@tiptap/extension-text-style'

export const MAIL_FONTS = [
  ['微软雅黑', 'Microsoft YaHei', 'Microsoft YaHei, PingFang SC, sans-serif'],
  ['宋体', 'SimSun', 'SimSun, Songti SC, serif'],
  ['黑体', 'SimHei', 'SimHei, Heiti SC, sans-serif'],
  ['楷体', 'KaiTi', 'KaiTi, Kaiti SC, serif'],
  ['仿宋', 'FangSong', 'FangSong, STFangsong, serif'],
  ['Arial', 'Arial', 'Arial, Helvetica, sans-serif'],
  ['Times New Roman', 'Times New Roman', 'Times New Roman, Times, serif'],
  ['Georgia', 'Georgia', 'Georgia, serif'],
  ['等宽字体', 'Monospace', 'Consolas, Menlo, monospace'],
] as const
export const MAIL_FONT_SIZES = [10, 12, 14, 16, 18, 20, 24, 28, 32, 36, 48, 64, 72, 96]

export type MailTextStyle = { fontFamily: string; fontSize: string; color: string; backgroundColor: string }
/** Empty means inherited; mixed selections must not silently display the first text's style. */
export function selectionTextStyle(editor: Editor): MailTextStyle {
  const keys = ['fontFamily', 'fontSize', 'color', 'backgroundColor'] as const
  const selected: Record<string, unknown>[] = []
  if (editor.state.selection.empty) selected.push(editor.getAttributes('textStyle'))
  else for (const range of editor.state.selection.ranges) editor.state.doc.nodesBetween(range.$from.pos, range.$to.pos, (node) => {
    if (node.isText) selected.push(node.marks.find((mark) => mark.type.name === 'textStyle')?.attrs ?? {})
  })
  return Object.fromEntries(keys.map((key) => {
    const values = new Set(selected.map((attrs) => String(attrs[key] || '')))
    return [key, values.size > 1 ? 'mixed' : [...values][0] || '']
  })) as MailTextStyle
}

// Markdown has no native font/color syntax. Serialize just the styled text as
// inline HTML, with a tokenizer so nested **bold**, links and code stay Markdown.
const MailTextStyle = TextStyle.extend({
  renderMarkdown(node, helpers) {
    const span = document.createElement('span')
    span.dataset.kojoomailStyle = '1'
    for (const [attr, property] of [['fontFamily', 'font-family'], ['fontSize', 'font-size'], ['color', 'color'], ['backgroundColor', 'background-color']]) {
      if (node.attrs?.[attr]) span.style.setProperty(property, node.attrs[attr])
    }
    if (!span.getAttribute('style')) return helpers.renderChildren(node)
    return `${span.outerHTML.slice(0, -7)}${helpers.renderChildren(node)}</span>`
  },
  parseMarkdown(token, helpers) {
    return helpers.applyMark('textStyle', helpers.parseInline(token.tokens || []), token.styleAttrs)
  },
  markdownTokenizer: {
    name: 'textStyle', level: 'inline',
    start: (src) => src.toLowerCase().indexOf('<span'),
    tokenize(src, _tokens, lexer) {
      const opening = /^<span\b[^>]*>/i.exec(src)?.[0]
      if (!opening || !/\bdata-kojoomail-style="1"/.test(opening)) return undefined
      const template = document.createElement('template')
      template.innerHTML = `${opening}</span>`
      const span = template.content.firstElementChild as HTMLElement | null
      if (!span) return undefined
      const styleAttrs = { fontFamily: span.style.fontFamily, fontSize: span.style.fontSize, color: span.style.color, backgroundColor: span.style.backgroundColor }
      const delimiters = /\\[\s\S]|`+|<\/?span\b[^>]*>/gi
      delimiters.lastIndex = opening.length
      let depth = 1, match
      while ((match = delimiters.exec(src))) {
        if (match[0].startsWith('\\')) continue
        if (match[0].startsWith('`')) {
          const code = /^(`+)([^`]|[^`][\s\S]*?[^`])\1(?!`)/.exec(src.slice(match.index))
          if (code) delimiters.lastIndex = match.index + code[0].length
          continue
        }
        depth += /^<\//.test(match[0]) ? -1 : 1
        if (depth === 0) {
          const text = src.slice(opening.length, match.index)
          return { type: 'textStyle', raw: src.slice(0, delimiters.lastIndex), text, styleAttrs, tokens: lexer.inlineTokens(text) }
        }
      }
      return undefined
    },
  },
}).configure({ HTMLAttributes: { 'data-kojoomail-style': '1' } })

const cellStyle = 'border:1px solid #b8c2cc;padding:8px 10px;vertical-align:top;text-align:left'
const tableOptions = {
  resizable: true,
  HTMLAttributes: { 'data-kojoomail-table': '1', border: '1', cellpadding: '8', cellspacing: '0', style: 'border-collapse:collapse;border:1px solid #b8c2cc' },
}
function baseExtensions() {
  return [
    StarterKit.configure({ link: { openOnClick: false, defaultProtocol: 'https', protocols: ['http', 'https', 'mailto', 'tel'] } }),
    MailTextStyle, FontFamily, FontSize, Color, BackgroundColor,
    TableRow,
    TableCell.configure({ HTMLAttributes: { style: cellStyle } }),
    TableHeader.configure({ HTMLAttributes: { style: `${cellStyle};background-color:#f2f4f7;color:#1f2a37` } }),
    Image, TaskList, TaskItem.configure({ nested: true }),
  ]
}

function needsHtmlTable(node: JSONContent) {
  return node.content?.some((row, i) => row.content?.some((cell) =>
    cell.attrs?.colwidth?.some((w: number) => w > 0) || cell.attrs?.colspan > 1 || cell.attrs?.rowspan > 1 ||
    cell.type !== (i === 0 ? 'tableHeader' : 'tableCell') || cell.content?.length !== 1 ||
    cell.content[0]?.type !== 'paragraph' || cell.content[0]?.content?.some((n) => n.type === 'hardBreak'),
  ))
}

// 普通表格保持 GFM；列宽、合并单元格、多段落不能用管道表格无损表达，使用 Markdown 允许的 HTML 块。
const MailTable = Table.extend({
  renderMarkdown(node, helpers, context) {
    if (needsHtmlTable(node)) {
      return generateHTML({ type: 'doc', content: [node] }, [...baseExtensions(), Table.configure(tableOptions)])
    }
    return this.parent?.(node, helpers, context) ?? ''
  },
})

export function mailEditorExtensions() {
  return [...baseExtensions(), MailTable.configure(tableOptions), Markdown.configure({ markedOptions: { gfm: true, breaks: true } })]
}
