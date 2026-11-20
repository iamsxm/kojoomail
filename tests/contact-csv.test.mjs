import { test } from 'node:test'
import assert from 'node:assert/strict'
import { parseContactCsv } from '../src/lib/contact-csv.ts'

test('兼容 GB18030 公司通讯录，保留工号前导零', () => {
  const csv = Buffer.from('d0d5c3fb2cb9a4bac52cd3cabcfeb5d8d6b70d0ad5c5c8fd2c30303132332c74657374406578616d706c652e636f6d', 'hex')
  assert.deepEqual(parseContactCsv(csv), [{ name: '张三', employeeId: '00123', email: 'test@example.com' }])
})

test('UTF-8 BOM、字段引号、内嵌换行和缺失邮箱的记录可正常读取', () => {
  const csv = Buffer.from('\uFEFFEmail Address,Display Name\r\na@example.com,"A, ""Example""\r\nTeam"\r\n,B\r\n\r\n')
  assert.deepEqual(parseContactCsv(csv), [
    { name: 'A, "Example"\r\nTeam', email: 'a@example.com', employeeId: '' },
    { name: 'B', email: '', employeeId: '' },
  ])
  assert.throws(() => parseContactCsv(Buffer.from('Name,Id\nA,1')), /email-column/)
  assert.throws(() => parseContactCsv(Buffer.from('Email\n"a@example.com')), /csv/)
})
