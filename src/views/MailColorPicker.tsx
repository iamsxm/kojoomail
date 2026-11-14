import { useId, useLayoutEffect, useRef, useState } from 'react'
import { L } from '../lib/i18n'

const COLORS = [
  ['#1f2a37', '墨黑', 'Ink'], ['#64748b', '灰色', 'Gray'], ['#ffffff', '白色', 'White'], ['#dc2626', '红色', 'Red'],
  ['#ea580c', '橙色', 'Orange'], ['#ca8a04', '金色', 'Gold'], ['#16a34a', '绿色', 'Green'], ['#0d9488', '青色', 'Teal'],
  ['#0284c7', '天蓝', 'Sky'], ['#2563eb', '蓝色', 'Blue'], ['#4f46e5', '靛蓝', 'Indigo'], ['#9333ea', '紫色', 'Purple'],
  ['#db2777', '粉色', 'Pink'], ['#fee2e2', '浅红', 'Light red'], ['#fef08a', '浅黄', 'Light yellow'], ['#dcfce7', '浅绿', 'Light green'],
  ['#dbeafe', '浅蓝', 'Light blue'], ['#f3e8ff', '浅紫', 'Light purple'],
] as const

function hexColor(value: string, fallback: string) {
  if (/^#[\da-f]{6}$/i.test(value)) return value.toLowerCase()
  if (/^#[\da-f]{3}$/i.test(value)) return '#' + value.slice(1).split('').map((c) => c + c).join('')
  const rgb = /^rgb\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*\)$/.exec(value)
  return rgb ? '#' + rgb.slice(1).map((n) => Number(n).toString(16).padStart(2, '0')).join('') : fallback
}

export default function MailColorPicker({ kind, value, open, onToggle, onApply }: {
  kind: 'text' | 'background'; value: string; open: boolean; onToggle: () => void; onApply: (color: string | null) => void
}) {
  const id = useId()
  const [custom, setCustom] = useState('')
  const [error, setError] = useState(false)
  const buttonRef = useRef<HTMLButtonElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)
  const [position, setPosition] = useState({ left: 12, top: 12 })
  useLayoutEffect(() => {
    if (!open) return
    const place = () => {
      const button = buttonRef.current?.getBoundingClientRect()
      const panel = panelRef.current?.getBoundingClientRect()
      if (!button || !panel) return
      const left = Math.max(12, Math.min(button.right - panel.width, window.innerWidth - panel.width - 12))
      const preferredTop = button.bottom + 6 + panel.height <= window.innerHeight - 12 ? button.bottom + 6 : button.top - panel.height - 6
      const top = Math.max(12, Math.min(preferredTop, window.innerHeight - panel.height - 12))
      setPosition((old) => old.left === left && old.top === top ? old : { left, top })
    }
    place()
    const resize = new ResizeObserver(place)
    if (panelRef.current) resize.observe(panelRef.current)
    window.addEventListener('resize', place)
    window.addEventListener('scroll', place, true)
    return () => { resize.disconnect(); window.removeEventListener('resize', place); window.removeEventListener('scroll', place, true) }
  }, [open])
  const text = kind === 'text'
  const label = text ? L('文字颜色', 'Text color') : L('文字背景色', 'Text background color')
  const current = hexColor(value, text ? '#1f2a37' : '#fef08a')
  const apply = (color: string | null) => { setError(false); onApply(color) }
  return <div className="mail-editor-popover" onKeyDown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); if (open) onToggle() } }}>
    <button ref={buttonRef} type="button" className="editor-tool editor-color-toggle" aria-label={label} title={label} aria-pressed={Boolean(value)} aria-expanded={open} aria-controls={open ? id : undefined}
      onMouseDown={(e) => e.preventDefault()} onClick={() => { setCustom(current); setError(false); onToggle() }}>
      <span aria-hidden="true" className={text ? 'editor-text-color' : 'editor-background-color'} style={value === 'mixed' ? undefined : text ? { borderColor: value || 'currentColor' } : { backgroundColor: value || '#fef08a', color: '#1f2a37' }}>A</span>
      <span aria-hidden="true" className="editor-color-chevron">▾</span>
    </button>
    {open && <div ref={panelRef} id={id} className="editor-popover color-popover" style={position} role="group" aria-label={label}>
      <div className="editor-color-heading">{label}{value === 'mixed' && <span>{L('选区含多种颜色', 'Mixed colors')}</span>}</div>
      <button type="button" className="btn sm editor-color-reset" onMouseDown={(e) => e.preventDefault()} onClick={() => apply(null)}>{text ? L('自动 · 跟随正文颜色', 'Automatic · inherit text color') : L('无背景色', 'No background color')}</button>
      <div className="editor-color-grid">
        {COLORS.map(([color, zh, en]) => <button key={color} type="button" className="editor-color-swatch" style={{ backgroundColor: color }} aria-label={`${L(zh, en)} ${color}`} title={`${L(zh, en)} ${color}`} aria-pressed={Boolean(value) && value !== 'mixed' && current === color}
          onMouseDown={(e) => e.preventDefault()} onClick={() => apply(color)} />)}
      </div>
      <form className="editor-custom-color" onSubmit={(e) => {
        e.preventDefault()
        if (!/^#(?:[\da-f]{3}|[\da-f]{6})$/i.test(custom.trim())) { setError(true); return }
        apply(hexColor(custom.trim(), current))
      }}>
        <label htmlFor={`${id}-hex`}>{L('自定义颜色', 'Custom color')}</label>
        <div className="editor-custom-color-row">
          <input type="color" aria-label={L('选取自定义颜色', 'Pick a custom color')} value={hexColor(custom, current)} onChange={(e) => { setCustom(e.target.value); setError(false) }} />
          <input id={`${id}-hex`} type="text" aria-label={L('自定义颜色值', 'Custom color value')} value={custom} maxLength={7} placeholder="#2563eb" spellCheck={false} aria-invalid={error || undefined} aria-describedby={error ? `${id}-error` : undefined} onChange={(e) => { setCustom(e.target.value); setError(false) }} />
          <button type="submit" className="btn sm">{L('应用', 'Apply')}</button>
        </div>
        {error && <span id={`${id}-error`} role="alert">{L('请输入 #RGB 或 #RRGGBB 颜色值', 'Enter a #RGB or #RRGGBB color')}</span>}
      </form>
    </div>}
  </div>
}
