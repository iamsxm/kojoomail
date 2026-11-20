import { useCallback, useEffect, useRef, useState } from 'react'
import { EditorContent, useEditor, useEditorState } from '@tiptap/react'
import { NodeSelection, type Transaction } from '@tiptap/pm/state'
import { mailEditorExtensions, MAIL_FONTS, MAIL_FONT_SIZES, selectionTextStyle } from '../lib/mail-editor'
import { imageByteLength, MAX_IMAGE_BYTES, MAX_INLINE_IMAGE_BYTES, readInlineImage } from '../lib/mail-image'
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
  const fileRef = useRef<HTMLInputElement>(null)
  const imageImport = useRef<AbortController | null>(null)
  const [readingImages, setReadingImages] = useState(false)
  const [imageError, setImageError] = useState('')
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
    image: e.state.selection instanceof NodeSelection && e.state.selection.node.type.name === 'image'
      ? { position: e.state.selection.from, width: e.state.selection.node.attrs.width, height: e.state.selection.node.attrs.height, alt: e.state.selection.node.attrs.alt }
      : null,
    undo: e.can().undo(), redo: e.can().redo(),
  } : null })
  useEffect(() => {
    if (!editor || !active || lastValue.current === value) return
    imageImport.current?.abort()
    editor.commands.setContent(value, { contentType: 'markdown', emitUpdate: false })
    lastValue.current = value
  }, [editor, value, active])
  useEffect(() => () => imageImport.current?.abort(), [active])
  if (!editor || !state) return null
  const insertImages = async (files: File[]) => {
    if (imageImport.current) return
    setImageError('')
    if (files.some((file) => file.size > MAX_IMAGE_BYTES)) {
      setImageError(L('每张正文图片最大 3 MB。', 'Each inline image can be up to 3 MB.'))
      return
    }
    const fits = () => {
      const sources = new Set<string>()
      editor.state.doc.descendants((node) => {
        if (node.type.name === 'image' && typeof node.attrs.src === 'string') sources.add(node.attrs.src)
      })
      const used = [...sources].reduce((sum, src) => sum + imageByteLength(src), 0)
      return used + files.reduce((sum, file) => sum + file.size, 0) <= MAX_INLINE_IMAGE_BYTES
    }
    const limitError = L('正文图片合计不能超过 10 MB。', 'Inline images can total up to 10 MB.')
    if (!fits()) { setImageError(limitError); return }
    const request = new AbortController()
    imageImport.current = request
    let bookmark = editor.state.selection.getBookmark()
    const trackSelection = ({ transaction }: { transaction: Transaction }) => { bookmark = bookmark.map(transaction.mapping) }
    editor.on('transaction', trackSelection)
    setReadingImages(true)
    try {
      const images = await Promise.all(files.map((file) => readInlineImage(file).catch(() => null)))
      if (request.signal.aborted || editor.isDestroyed) return
      if (!fits()) { setImageError(limitError); return }
      const valid = images.filter((image) => image !== null)
      if (valid.length !== images.length) setImageError(L('部分图片无法读取。支持 PNG、JPEG、GIF 和 WebP。', 'Some images could not be read. Supported formats are PNG, JPEG, GIF and WebP.'))
      if (!valid.length) return
      const { from, to } = bookmark.resolve(editor.state.doc)
      editor.chain().focus().insertContentAt({ from, to }, valid).run()
    } finally {
      editor.off('transaction', trackSelection)
      imageImport.current = null
      if (!editor.isDestroyed) setReadingImages(false)
    }
  }
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
      {tool(L('插入图片', 'Insert image'), <Icon name="image" />, () => fileRef.current?.click(), undefined, readingImages)}
      <input ref={fileRef} type="file" accept="image/png,image/jpeg,image/gif,image/webp,.jpg,.jpeg,.png,.gif,.webp" multiple hidden onChange={(e) => {
        const files = Array.from(e.currentTarget.files ?? [])
        e.currentTarget.value = ''
        if (files.length) void insertImages(files)
      }} />
      {tool(L('清除格式', 'Clear formatting'), 'Tx', () => { editor.chain().focus().unsetAllMarks().clearNodes().run() })}
    </div>
    {state.image && <form key={JSON.stringify(state.image)} className="editor-image-tools" aria-label={L('图片设置', 'Image settings')} onSubmit={(e) => {
      e.preventDefault()
      const data = new FormData(e.currentTarget)
      const width = Number(data.get('width')) || null
      editor.chain().focus().updateAttributes('image', {
        width, height: width === Number(state.image?.width) ? state.image?.height : null, alt: String(data.get('alt') ?? ''),
      }).run()
    }}>
      <label>{L('图片宽度', 'Image width')} <input name="width" type="number" min="1" step="1" defaultValue={state.image.width ?? ''} placeholder={L('原始', 'Original')} /> px</label>
      <label>{L('替代文字', 'Alt text')} <input name="alt" defaultValue={state.image.alt ?? ''} /></label>
      <button type="submit" className="btn sm">{L('应用', 'Apply')}</button>
      <button type="button" className="btn sm" onClick={() => editor.chain().focus().updateAttributes('image', { width: null, height: null }).run()}>{L('恢复原始尺寸', 'Reset size')}</button>
      <button type="button" className="btn sm" onClick={() => editor.chain().focus().deleteSelection().run()}>{L('删除图片', 'Remove image')}</button>
    </form>}
    {readingImages && <div className="editor-image-status" role="status">{L('正在读取图片…', 'Reading images…')}</div>}
    {imageError && <div className="editor-image-status" role="alert">{imageError}</div>}
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
    <div className="editor-scroll" onPasteCapture={(e) => {
      const clipboard = e.clipboardData
      const candidates = clipboard.files.length ? Array.from(clipboard.files) : Array.from(clipboard.items).flatMap((item) => {
        const file = item.getAsFile()
        return file ? [file] : []
      })
      const files = candidates.filter((file) => file.type.startsWith('image/') || /\.(png|jpe?g|gif|webp)$/i.test(file.name))
      if (!files.length) return
      e.preventDefault()
      e.stopPropagation()
      void insertImages(files)
    }}><EditorContent editor={editor} /></div>
  </div>
}
