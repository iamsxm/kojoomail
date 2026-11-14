function isEditorStyle(el: Element) {
  if (el.tagName !== 'SPAN' || el.getAttribute('data-kojoomail-style') !== '1') return false
  if ([...el.attributes].some((a) => !['data-kojoomail-style', 'style'].includes(a.name))) return false
  const css = (el as HTMLElement).style
  const color = /^(#[\da-f]{3,8}|rgb\(\s*\d{1,3}\s*,\s*\d{1,3}\s*,\s*\d{1,3}\s*\))$/i
  const rules: Record<string, RegExp> = {
    'font-family': /^[\p{L}\p{N} _,'"-]+$/u,
    'font-size': /^(?:[89]|[1-8]\d|9[0-6])px$/,
    color, 'background-color': color,
  }
  // Check the raw declarations too: CSSOM drops unknown/invalid declarations,
  // which must not let a hand-edited marker bypass source-mode protection.
  const raw = el.getAttribute('style') || ''
  if (raw.split(';').some((part) => {
    if (!part.trim()) return false
    const colon = part.indexOf(':')
    const property = part.slice(0, colon).trim().toLowerCase()
    return colon < 0 || !rules[property]?.test(part.slice(colon + 1).trim()) || !css.getPropertyValue(property)
  })) return false
  return css.length > 0 && Array.from({ length: css.length }, (_, i) => css.item(i)).every((property) =>
    !css.getPropertyPriority(property) && rules[property]?.test(css.getPropertyValue(property)),
  )
}

function isEditorTable(html: string) {
  // template 不挂到文档上，不执行正文里的脚本或加载资源。
  const template = document.createElement('template')
  template.innerHTML = html
  const tags: Record<string, string[]> = {
    TABLE: ['data-kojoomail-table', 'border', 'cellpadding', 'cellspacing'], TBODY: [], THEAD: [], TR: [],
    TD: ['colspan', 'rowspan', 'colwidth', 'align'], TH: ['colspan', 'rowspan', 'colwidth', 'align'], COLGROUP: [], COL: [],
    P: [], STRONG: [], B: [], EM: [], I: [], U: [], S: [], DEL: [], BR: [], CODE: [], PRE: [], SPAN: [],
    UL: [], OL: ['start'], LI: [], BLOCKQUOTE: [], A: ['href', 'target', 'rel'], IMG: ['src', 'alt', 'title', 'width', 'height'],
  }
  const styles: Record<string, Record<string, RegExp>> = {
    TABLE: { 'border-collapse': /^collapse$/, border: /^1px solid (?:#b8c2cc|rgb\(184, 194, 204\))$/, width: /^\d+(\.\d+)?px$/, 'min-width': /^\d+(\.\d+)?px$/ },
    TD: { border: /^1px solid (?:#b8c2cc|rgb\(184, 194, 204\))$/, padding: /^8px 10px$/, 'vertical-align': /^top$/, 'text-align': /^(left|center|right)$/ },
    TH: { border: /^1px solid (?:#b8c2cc|rgb\(184, 194, 204\))$/, padding: /^8px 10px$/, 'vertical-align': /^top$/, 'text-align': /^(left|center|right)$/, 'background-color': /^(#f2f4f7|rgb\(242, 244, 247\))$/, color: /^(#1f2a37|rgb\(31, 42, 55\))$/ },
    COL: { width: /^\d+(\.\d+)?px$/, 'min-width': /^\d+(\.\d+)?px$/ },
  }
  return [...template.content.querySelectorAll('*')].every((el) => {
    if (el.tagName === 'SPAN') return isEditorStyle(el)
    const allowed = tags[el.tagName]
    if (!allowed) return false
    return [...el.attributes].every((attr) => {
      if (attr.name !== 'style') return allowed.includes(attr.name)
      const css = (el as HTMLElement).style
      return Array.from({ length: css.length }, (_, i) => css.item(i)).every((property) => styles[el.tagName]?.[property]?.test(css.getPropertyValue(property)))
    })
  })
}

/** 自定义 HTML 默认进入源码模式；已支持的编辑器样式可以直接重开。 */
export function needsSourceMode(source: string) {
  const custom = source
    .replace(/<table\b(?=[^>]*data-kojoomail-table="1")[\s\S]*?<\/table>/gi, (table) => isEditorTable(table) ? '' : table)
    .replace(/<span\b(?=[^>]*data-kojoomail-style="1")([^>]*)>([\s\S]*?)<\/span>/gi, (html, _attrs, content) => {
      const template = document.createElement('template')
      template.innerHTML = html
      const el = template.content.firstElementChild
      return el && isEditorStyle(el) ? content : html
    })
    // Overlapping Markdown marks may serialize as plain inline HTML tags.
    // Only strip attribute-free tags that the editor can restore losslessly.
    .replace(/<\/?(?:u|strong|b|em|i|s|del)>|<br\s*\/?>/gi, '')
  return /<\/?[a-z][^>]*>/i.test(custom)
}
