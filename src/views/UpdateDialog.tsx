import { useEffect, useRef } from 'react'
import { createPortal } from 'react-dom'
import { Icon } from '../icons'
import { L } from '../lib/i18n'
import { useApp } from '../store'
import { MdView } from '../ui'

export default function UpdateDialog() {
  const info = useApp((s) => s.updateInfo)
  return info ? <UpdatePrompt key={info.version} /> : null
}

function UpdatePrompt() {
  const info = useApp((s) => s.updateInfo)!
  const version = useApp((s) => s.version)
  const busy = useApp((s) => s.updateBusy)
  const error = useApp((s) => s.updateError)
  const dismiss = useApp((s) => s.dismissUpdate)
  const skip = useApp((s) => s.skipUpdate)
  const install = useApp((s) => s.installUpdate)
  const ref = useRef<HTMLDialogElement>(null)
  const laterRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    const dialog = ref.current!
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null
    dialog.showModal()
    laterRef.current?.focus()
    return () => {
      dialog.close()
      if (previous?.isConnected) previous.focus()
    }
  }, [])

  return createPortal(
    <dialog ref={ref} className="update-dialog sq" aria-labelledby="update-title" aria-describedby="update-description" onCancel={(e) => { e.preventDefault(); dismiss() }} onKeyDown={(e) => e.stopPropagation()}>
      <header className="update-heading">
        <h2 id="update-title">{L(`Kojoo Mail ${info.version} 可更新`, `Kojoo Mail ${info.version} is available`)}</h2>
        <button type="button" className="tb" aria-label={L('关闭，稍后更新', 'Close and update later')} disabled={busy} onClick={dismiss}><Icon name="x" size={16} /></button>
      </header>
      <p id="update-description" className="update-description">{L(`当前版本 ${version}。安装前会先退出 Kojoo Mail，完成后自动重新打开。`, `Current version ${version}. Kojoo Mail will close for installation and reopen when the update is complete.`)}</p>
      <section className="update-notes" aria-label={L('更新说明', 'Release notes')} tabIndex={0}>
        {info.notes.trim() ? <MdView className="update-notes-content" md={info.notes} /> : <p className="update-empty">{L('此版本未提供更新说明。', 'No release notes were provided for this version.')}</p>}
      </section>
      {busy && <p className="update-status" role="status">{L('正在下载并安装更新，完成后将自动重启…', 'Downloading and installing the update. The app will restart when it is ready…')}</p>}
      {error && <p className="update-error" role="alert">{error}</p>}
      <footer className="update-actions">
        <button type="button" className="btn sq update-skip" disabled={busy} onClick={() => void skip()}>{L('跳过此版本', 'Skip this version')}</button>
        <button ref={laterRef} type="button" className="btn sq" disabled={busy} onClick={dismiss}>{L('稍后', 'Later')}</button>
        <button type="button" className="btn primary sq" disabled={busy} onClick={() => void install(info.version)}>{busy ? L('正在安装…', 'Installing…') : error ? L('重试安装', 'Retry installation') : L('安装并重启', 'Install and restart')}</button>
      </footer>
    </dialog>,
    document.body,
  )
}
