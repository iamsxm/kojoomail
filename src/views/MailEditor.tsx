import { useCallback, useEffect, useRef, useState } from 'react'
import { EditorContent, useEditor, useEditorState } from '@tiptap/react'
import { mailEditorExtensions, MAIL_FONTS, MAIL_FONT_SIZES, selectionTextStyle } from '../lib/mail-editor'
import { L } from '../lib/i18n'
import { Icon } from '../icons'
import { useEditorPopovers } from '../lib/use-editor-popovers'
import MailColorPicker from './MailColorPicker'

const fontKey = (value: string) => value.replace(/[\s'"]/g, '').toLowerCase()

export default function MailEditor({ value, onChange, autoFocus, active }: { value: string; onChange: (value: string) => void; autoFocus: boolean; active: boolean }) {
  const lastValue = useRef(value)
  const [grid, setGrid] = useState([3, 3])
  const [url, setUrl] = useState('')
  const [linkError, setLinkError] = useState('')
  const widthRef = useRef<HTMLInputElement>(null)
  const editor = useEditor({
    extensions: mailEditorExtensions(),
    content: value, contentType: 'markdown',
    autofocus: autoFocus ? 'end' : false,
    shouldRerenderOnTransaction: false,
    editorProps: { attributes: { class: 'mail-document', role: 'textbox', 'aria-label': L('可视化正文', 'Visual message body'), 'aria-multiline': 'true', spellcheck: 'false' } },
    onUpdate: ({ editor }) => {
      const markdown = editor.getMarkdown()
      lastValue.current = markdown
      onChange(markdown)
    },
  })
  const restoreEditorFocus = useCallback(() => { editor?.commands.focus() }, [editor])
  const { tableOpen, setTableOpen, linkOpen, setLinkOpen, colorOpen, setColorOpen } = useEditorPopovers(active, restoreEditorFocus)
  const state = useEditorState({ editor, selector: ({ editor: e }) => e ? {
    bold: e.isActive('bold'), italic: e.isActive('italic'), underline: e.isActive('underline'), strike: e.isActive('strike'),
    bullet: e.isActive('bulletList'), ordered: e.isActive('orderedList'), quote: e.isActive('blockquote'),
    link: e.isActive('link'), table: e.isActive('table'), heading: e.getAttributes('heading').level || 0,
    textStyle: selectionTextStyle(e),
    cell: e.state.selection.$from.start(),
    cellAttrs: e.isActive('tableHeader') ? e.getAttributes('tableHeader') : e.getAttributes('tableCell'),
    undo: e.can().undo(), redo: e.can().redo(),
  } : null })
  useEffect(() => {
    if (!editor || !active || lastValue.current === value) return
    editor.commands.setContent(value, { contentType: 'markdown', emitUpdate: false })
    lastValue.current = value
  }, [editor, value, active])
  if (!editor || !state) return null
  const fontValue = MAIL_FONTS.find((font) => fontKey(font[2]) === fontKey(state.textStyle.fontFamily))?.[2] ?? state.textStyle.fontFamily
  const applyColumnWidth = () => {
    const raw = widthRef.current?.value || ''
    const width = raw ? Math.max(40, Math.min(1600, Math.round(Number(raw)))) : null
    if (width !== null && !Number.isFinite(width)) return
    const next = width === null ? null : Array(state.cellAttrs.colspan || 1).fill(width)
    if (JSON.stringify(next) === JSON.stringify(state.cellAttrs.colwidth ?? null)) return
    editor.chain().focus().setCellAttribute('colwidth', next).run()
  }
  const tool = (label: string, text: React.ReactNode, run: () => void, active?: boolean, disabled = false) => (
    <button type="button" className="editor-tool" aria-label={label} title={label} aria-pressed={active} disabled={disabled} onMouseDown={(e) => e.preventDefault()} onClick={run}>{text}</button>
  )
  return <div className="mail-editor">
    <div className="editor-toolbar" role="toolbar" aria-label={L('正文格式', 'Message formatting')}>
      {tool(L('撤销', 'Undo'), <Icon name="undo" />, () => { editor.chain().focus().undo().run() }, undefined, !state.undo)}
      {tool(L('重做', 'Redo'), <Icon name="undo" style={{ transform: 'scaleX(-1)' }} />, () => { editor.chain().focus().redo().run() }, undefined, !state.redo)}
      <select aria-label={L('段落样式', 'Paragraph style')} value={state.heading} onChange={(e) => {
        const level = Number(e.target.value) as 1 | 2 | 3 | 4 | 5 | 6
        if (level) editor.chain().focus().setHeading({ level }).run()
        else editor.chain().focus().setParagraph().run()
      }}>
        <option value="0">{L('正文', 'Normal')}</option>{[1, 2, 3, 4, 5, 6].map((level) => <option key={level} value={level}>{L(`标题 ${level}`, `Heading ${level}`)}</option>)}
      </select>
      <select className="editor-font-family" aria-label={L('字体', 'Font family')} title={L('字体（使用收件人设备上的可用字体）', 'Font family (uses fonts available on the recipient’s device)')} value={fontValue} onChange={(e) => {
        const chain = editor.chain().focus()
        if (e.target.value) chain.setFontFamily(e.target.value).run()
        else chain.unsetFontFamily().run()
      }}>
        <option value="">{L('默认字体', 'Default font')}</option>
        {fontValue === 'mixed' && <option value="mixed" disabled>{L('混合字体', 'Mixed fonts')}</option>}
        {fontValue && fontValue !== 'mixed' && !MAIL_FONTS.some((f) => f[2] === fontValue) && <option value={fontValue}>{fontValue}</option>}
        {MAIL_FONTS.map(([zh, en, family]) => <option key={family} value={family} style={{ fontFamily: family }}>{L(zh, en)}</option>)}
      </select>
      <select className="editor-font-size" aria-label={L('字号', 'Font size')} title={L('发送字号（像素）', 'Sent font size (pixels)')} value={state.textStyle.fontSize} onChange={(e) => {
        const chain = editor.chain().focus()
        if (e.target.value) chain.setFontSize(e.target.value).run()
        else chain.unsetFontSize().run()
      }}>
        <option value="">{L('默认字号', 'Default size')}</option>
        {state.textStyle.fontSize === 'mixed' && <option value="mixed" disabled>{L('混合字号', 'Mixed sizes')}</option>}
        {state.textStyle.fontSize && state.textStyle.fontSize !== 'mixed' && !MAIL_FONT_SIZES.some((n) => `${n}px` === state.textStyle.fontSize) && <option value={state.textStyle.fontSize}>{state.textStyle.fontSize}</option>}
        {MAIL_FONT_SIZES.map((size) => <option key={size} value={`${size}px`}>{size} px</option>)}
      </select>
      {tool(L('粗体', 'Bold'), <b>B</b>, () => { editor.chain().focus().toggleBold().run() }, state.bold)}
      {tool(L('斜体', 'Italic'), <i>I</i>, () => { editor.chain().focus().toggleItalic().run() }, state.italic)}
      {tool(L('下划线', 'Underline'), <u>U</u>, () => { editor.chain().focus().toggleUnderline().run() }, state.underline)}
      {tool(L('删除线', 'Strikethrough'), <s>S</s>, () => { editor.chain().focus().toggleStrike().run() }, state.strike)}
      {(['text', 'background'] as const).map((kind) => <MailColorPicker key={kind} kind={kind} value={kind === 'text' ? state.textStyle.color : state.textStyle.backgroundColor} open={colorOpen === kind}
        onToggle={() => { setColorOpen(colorOpen === kind ? null : kind); setLinkOpen(false); setTableOpen(false) }}
        onApply={(color) => {
          const chain = editor.chain().focus()
          if (kind === 'text') { if (color) chain.setColor(color).run(); else chain.unsetColor().run() }
          else { if (color) chain.setBackgroundColor(color).run(); else chain.unsetBackgroundColor().run() }
          setColorOpen(null)
        }} />)}
      <span className="editor-divider" />
      {tool(L('无序列表', 'Bullet list'), '• ≡', () => { editor.chain().focus().toggleBulletList().run() }, state.bullet)}
      {tool(L('有序列表', 'Numbered list'), '1. ≡', () => { editor.chain().focus().toggleOrderedList().run() }, state.ordered)}
      {tool(L('引用', 'Quote'), '❝', () => { editor.chain().focus().toggleBlockquote().run() }, state.quote)}
      <div className="mail-editor-popover" onKeyDown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); setLinkOpen(false) } }}>
        {tool(L('插入链接', 'Insert link'), <Icon name="external" />, () => { setUrl(editor.getAttributes('link').href || 'https://'); setLinkError(''); setLinkOpen(!linkOpen); setTableOpen(false); setColorOpen(null) }, state.link)}
        {linkOpen && <form className="editor-popover link-popover" aria-label={L('编辑链接', 'Edit link')} onSubmit={(e) => {
          e.preventDefault()
          const href = url.trim()
          if (href && !/^(https?:\/\/\S+|mailto:[^\s@]+@[^\s@]+|tel:[+\d][\d ()-]*)$/i.test(href)) { setLinkError(L('请输入完整的 https://、mailto: 或 tel: 地址', 'Enter a complete https://, mailto: or tel: address')); return }
          const chain = editor.chain().focus().extendMarkRange('link')
          if (!href) chain.unsetLink().run()
          else if (editor.state.selection.empty && !state.link) chain.insertContent({ type: 'text', text: href, marks: [{ type: 'link', attrs: { href } }] }).run()
          else chain.setLink({ href }).run()
          setLinkOpen(false)
        }}>
          <label>{L('链接地址（留空移除）', 'Link address (empty to remove)')}<input autoFocus value={url} onChange={(e) => { setUrl(e.target.value); setLinkError('') }} /></label>
          {linkError && <span role="alert">{linkError}</span>}
          <button type="submit" className="btn primary sm">{L('应用', 'Apply')}</button>
        </form>}
      </div>
      <div className="mail-editor-popover" onKeyDown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); setTableOpen(false) } }}>
        {tool(L('插入表格', 'Insert table'), <Icon name="table" />, () => { setTableOpen(!tableOpen); setLinkOpen(false); setColorOpen(null) }, tableOpen)}
        {tableOpen && <div className="editor-popover table-popover" role="group" aria-label={L('选择表格大小', 'Choose table size')}>
          <span aria-live="polite">{L(`插入表格 · ${grid[0]} 行 × ${grid[1]} 列`, `Insert table · ${grid[0]} rows × ${grid[1]} columns`)}</span>
          <div className="table-picker">
            {Array.from({ length: 48 }, (_, i) => {
              const row = Math.floor(i / 8) + 1, col = i % 8 + 1
              return <button key={i} type="button" className={row <= grid[0] && col <= grid[1] ? 'selected' : ''} aria-label={L(`${row} 行 ${col} 列`, `${row} rows ${col} columns`)} onMouseDown={(e) => e.preventDefault()} onMouseEnter={() => setGrid([row, col])} onFocus={() => setGrid([row, col])} onClick={() => { editor.chain().focus().insertTable({ rows: row, cols: col, withHeaderRow: true }).run(); setTableOpen(false) }} />
            })}
          </div>
        </div>}
      </div>
      {tool(L('清除格式', 'Clear formatting'), 'Tx', () => { editor.chain().focus().unsetAllMarks().clearNodes().run() })}
    </div>
    {state.table && <div className="editor-table-tools" role="toolbar" aria-label={L('表格操作', 'Table actions')}>
      {([
        [L('加行', 'Add row'), () => editor.chain().focus().addRowAfter().run()],
        [L('加列', 'Add column'), () => editor.chain().focus().addColumnAfter().run()],
        [L('删行', 'Delete row'), () => editor.chain().focus().deleteRow().run()],
        [L('删列', 'Delete column'), () => editor.chain().focus().deleteColumn().run()],
        [L('合并 / 拆分', 'Merge / split'), () => editor.chain().focus().mergeOrSplit().run()],
        [L('删除表格', 'Delete table'), () => editor.chain().focus().deleteTable().run()],
      ] as [string, () => unknown][]).map(([label, run]) => <button key={label} type="button" className="btn sm" onMouseDown={(e) => e.preventDefault()} onClick={run}>{label}</button>)}
      <label className="editor-column-width">{L('列宽', 'Width')}
        <input ref={widthRef} key={`${state.cell}:${state.cellAttrs.colwidth?.join(',')}`} type="number" min={40} max={1600} step={10} defaultValue={state.cellAttrs.colwidth?.[0] ?? ''} placeholder={L('自动', 'Auto')} aria-label={L('列宽（像素，留空自动）', 'Column width (pixels, empty for auto)')} onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); applyColumnWidth() } }} />
        <span>px</span>
      </label>
      <button type="button" className="btn sm" onMouseDown={(e) => e.preventDefault()} onClick={applyColumnWidth}>{L('应用列宽', 'Apply width')}</button>
      <span>{L('拖动列边缘调整宽度 · Tab 切换单元格', 'Drag column edges to resize · Tab moves between cells')}</span>
    </div>}
    <div className="editor-scroll"><EditorContent editor={editor} /></div>
  </div>
}
