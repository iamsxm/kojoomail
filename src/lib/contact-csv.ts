export type ContactRow = { name: string; email: string; employeeId: string }
export const MAX_CONTACT_FILE_BYTES = 5 * 1024 * 1024

function decodeCsv(bytes: Uint8Array) {
  const encoding = bytes[0] === 0xff && bytes[1] === 0xfe ? 'utf-16le'
    : bytes[0] === 0xfe && bytes[1] === 0xff ? 'utf-16be' : 'utf-8'
  try { return new TextDecoder(encoding, { fatal: true }).decode(bytes) }
  catch {
    if (encoding === 'utf-8') {
      try { return new TextDecoder('gb18030', { fatal: true }).decode(bytes) } catch { /* report unsupported input below */ }
    }
    throw new Error('encoding')
  }
}

function csvRows(source: string) {
  const rows: string[][] = []
  let row: string[] = []
  let field = ''
  let quoted = false
  let closed = false
  const endField = () => { row.push(field.trim()); field = ''; closed = false }
  const endRow = () => {
    endField()
    if (row.some(Boolean)) rows.push(row)
    if (rows.length > 20_001) throw new Error('row-limit')
    row = []
  }
  for (let index = 0; index < source.length; index++) {
    const char = source[index]
    if (quoted) {
      if (char !== '"') field += char
      else if (source[index + 1] === '"') { field += char; index++ }
      else { quoted = false; closed = true }
    } else if (char === ',') endField()
    else if (char === '\r' || char === '\n') {
      endRow()
      if (char === '\r' && source[index + 1] === '\n') index++
    } else if (char === '"' && !closed && !field) quoted = true
    else if (char === '"' || (closed && !/\s/.test(char))) throw new Error('csv')
    else if (!closed) field += char
  }
  if (quoted) throw new Error('csv')
  endRow()
  return rows
}

export function parseContactCsv(bytes: Uint8Array): ContactRow[] {
  if (bytes.byteLength > MAX_CONTACT_FILE_BYTES) throw new Error('file-limit')
  const [header, ...rows] = csvRows(decodeCsv(bytes))
  const keys = header?.map((key) => key.toLowerCase().replace(/[\s_-]/g, '')) ?? []
  const column = (...aliases: string[]) => aliases.map((alias) => keys.indexOf(alias)).find((index) => index >= 0) ?? -1
  const email = column('邮件地址', '邮箱地址', '电子邮件地址', '电子邮件', '邮箱', 'email', 'emailaddress', 'email1value', 'email1address')
  if (email < 0) throw new Error('email-column')
  const name = column('姓名', '显示名称', '名称', '联系人', 'name', 'fullname', 'displayname')
  const employee = column('工号', '员工编号', '员工号', 'employeeid', 'employeenumber')
  if (!rows.length) throw new Error('empty')
  return rows.map((row) => ({ name: row[name] ?? '', email: row[email] ?? '', employeeId: row[employee] ?? '' }))
}
