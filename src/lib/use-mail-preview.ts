import { useEffect, useState } from 'react'
import type { MailContent, RenderedMail } from './mail-content'

type Preview = { key: string; html: string; error: unknown; pending: boolean }

/** 请求失效或组件卸载后忽略结果，防止旧正文覆盖最新预览。 */
export function useMailPreview(content: MailContent, render: (content: MailContent) => Promise<RenderedMail>) {
  const key = JSON.stringify(content)
  const [result, setResult] = useState<Preview>({ key: '', html: '', error: null, pending: true })
  useEffect(() => {
    let active = true
    render(JSON.parse(key)).then(
      (out) => { if (active) setResult({ key, html: out.preview, error: null, pending: false }) },
      (error: unknown) => { if (active) setResult({ key, html: '', error, pending: false }) },
    )
    return () => { active = false }
  }, [key, render])
  return result.key === key ? result : { key, html: '', error: null, pending: true }
}
