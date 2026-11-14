import { useEffect, useState } from 'react'

export function useEditorPopovers(active: boolean, onEscape?: () => void) {
  const [tableOpen, setTableOpen] = useState(false)
  const [linkOpen, setLinkOpen] = useState(false)
  const [colorOpen, setColorOpen] = useState<'text' | 'background' | null>(null)
  useEffect(() => {
    if (!active) { setTableOpen(false); setLinkOpen(false); setColorOpen(null); return }
    if (!tableOpen && !linkOpen && !colorOpen) return
    const close = () => { setTableOpen(false); setLinkOpen(false); setColorOpen(null) }
    const outside = (e: PointerEvent) => { if (!(e.target as Element).closest('.mail-editor-popover')) close() }
    // 工具按钮保留正文焦点，Esc 经常来自正文，必须在弹层关闭草稿之前处理。
    const key = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || document.querySelector('dialog[open]')) return
      e.preventDefault(); e.stopPropagation(); close(); onEscape?.()
    }
    window.addEventListener('pointerdown', outside)
    window.addEventListener('keydown', key, true)
    return () => { window.removeEventListener('pointerdown', outside); window.removeEventListener('keydown', key, true) }
  }, [active, tableOpen, linkOpen, colorOpen, onEscape])
  return { tableOpen, setTableOpen, linkOpen, setLinkOpen, colorOpen, setColorOpen }
}
