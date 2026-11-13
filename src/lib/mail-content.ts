/** 与后端 html::MailContent 对应；保持原始片段，预览和发送都由后端渲染。 */
export type MailQuote = { header: string; text: string; html: string; reply: boolean }
export type MailContent = { body: string; signature: string; quote?: MailQuote | null }
export type RenderedMail = { html: string; text: string; preview: string }
