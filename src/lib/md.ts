// 写信用 Markdown：发送时转成排版好的 HTML（纯文本部分保留原文）。签名是拼在正文里一起转的，所以格式和正文完全一样
import { marked } from 'marked'
import DOMPurify from 'dompurify'

marked.setOptions({ gfm: true, breaks: true })

// <style> 整段去掉：预览是直接放进界面里的，留着会把界面自己的样式也改掉（从别处粘来的 HTML 签名常带一段）。
// 发出去的也一样去掉，预览和实际发的才一致；样式写在标签的 style 属性里，各家邮箱也只认这种
const render = (md: string) => DOMPurify.sanitize(marked.parse(md, { async: false }) as string, { USE_PROFILES: { html: true }, FORBID_TAGS: ['style'] })

export function mdToHtml(md: string) {
  return `<div style="font-family:-apple-system,'Segoe UI','Microsoft YaHei',sans-serif;font-size:14px;line-height:1.6;color:#1F2A37">${render(md)}</div>`
}

export function mdPreview(md: string) {
  return render(md)
}

/** 回复时引用原文 */
export function quoteText(text: string, header: string) {
  const lines = text.split(/\r?\n/).slice(0, 200).map((l) => `> ${l}`)
  return `\n\n${header}\n${lines.join('\n')}`
}
