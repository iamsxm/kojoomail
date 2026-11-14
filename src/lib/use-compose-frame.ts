import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent, type CSSProperties } from 'react'

type Frame = { left: number; top: number; width: number; height: number }
const gap = 8
const initialFrame = (): Frame => {
  const width = Math.min(1120, innerWidth - gap * 2)
  const height = Math.min(820, innerHeight - 64)
  return { left: (innerWidth - width) / 2, top: Math.max(gap, (innerHeight - height) / 2), width, height }
}
function fit(f: Frame): Frame {
  const width = Math.max(Math.min(620, innerWidth - gap * 2), Math.min(f.width, innerWidth - gap * 2))
  const height = Math.max(Math.min(420, innerHeight - gap * 2), Math.min(f.height, innerHeight - gap * 2))
  return { width, height, left: Math.max(gap, Math.min(f.left, innerWidth - width - gap)), top: Math.max(gap, Math.min(f.top, innerHeight - height - gap)) }
}

/** 只移动写信面板；缩小应用窗口后仍保证标题和尺寸手柄可触达。 */
export function useComposeFrame() {
  const [frame, setFrame] = useState(initialFrame)
  const [maximized, setMaximized] = useState(false)
  const cleanup = useRef<(() => void) | null>(null)
  useEffect(() => {
    const resized = () => setFrame((f) => fit(f))
    window.addEventListener('resize', resized)
    return () => { window.removeEventListener('resize', resized); cleanup.current?.() }
  }, [])
  const toggleMaximize = () => setMaximized((v) => !v)
  const reset = () => { setMaximized(false); setFrame(initialFrame()) }
  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || maximized) return
    const target = e.target as HTMLElement
    const resizing = !!target.closest('[data-compose-resize]')
    if (!resizing && !target.closest('[data-compose-drag]')) return
    e.preventDefault()
    const start = { x: e.clientX, y: e.clientY, ...frame }
    cleanup.current?.()
    const move = (p: globalThis.PointerEvent) => {
      if (p.pointerId !== e.pointerId) return
      const x = p.clientX - start.x, y = p.clientY - start.y
      setFrame(fit(resizing
        ? { ...frame, width: start.width + x, height: start.height + y }
        : { ...frame, left: start.left + x, top: start.top + y }))
    }
    const end = (p: globalThis.PointerEvent) => { if (p.pointerId === e.pointerId) cleanup.current?.() }
    const stop = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', end)
      window.removeEventListener('pointercancel', end)
      window.removeEventListener('blur', stop)
      cleanup.current = null
    }
    cleanup.current = stop
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', end)
    window.addEventListener('pointercancel', end)
    window.addEventListener('blur', stop)
  }
  const onKeyDown = (e: KeyboardEvent<HTMLElement>, resize = false) => {
    if (!resize && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); toggleMaximize(); return }
    if (e.key === 'Home') { e.preventDefault(); reset(); return }
    if (maximized || !['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(e.key)) return
    e.preventDefault()
    const step = e.shiftKey ? 50 : 10
    const x = e.key === 'ArrowLeft' ? -step : e.key === 'ArrowRight' ? step : 0
    const y = e.key === 'ArrowUp' ? -step : e.key === 'ArrowDown' ? step : 0
    setFrame((f) => fit(resize ? { ...f, width: f.width + x, height: f.height + y } : { ...f, left: f.left + x, top: f.top + y }))
  }
  const style: CSSProperties = maximized
    ? { position: 'fixed', inset: gap, width: 'auto', height: 'auto', maxHeight: 'none', animation: 'none' }
    : { position: 'fixed', ...frame, maxHeight: 'none', animation: 'none' }
  return { style, maximized, toggleMaximize, reset, onPointerDown, onKeyDown }
}
