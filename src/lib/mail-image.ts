export const MAX_IMAGE_BYTES = 3_000_000
export const MAX_INLINE_IMAGE_BYTES = 10_000_000

export function imageByteLength(src: string) {
  const comma = src.indexOf(',')
  if (!/^data:image\//i.test(src) || comma < 0) return 0
  const length = src.length - comma - 1
  const padding = src.endsWith('==') ? 2 : src.endsWith('=') ? 1 : 0
  return Math.floor(length * 3 / 4) - padding
}

export async function readInlineImage(file: File) {
  const header = new Uint8Array(await file.slice(0, 12).arrayBuffer())
  const ascii = String.fromCharCode(...header)
  const mime = ascii.startsWith('\x89PNG\r\n\x1a\n') ? 'image/png'
    : ascii.startsWith('\xff\xd8\xff') ? 'image/jpeg'
    : /^GIF8[79]a/.test(ascii) ? 'image/gif'
    : ascii.startsWith('RIFF') && ascii.slice(8) === 'WEBP' ? 'image/webp'
    : null
  if (!mime) return null
  const src = await new Promise<string>((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result))
    reader.onerror = () => reject(reader.error)
    reader.readAsDataURL(file.slice(0, file.size, mime))
  })
  const image = new Image()
  image.src = src
  await image.decode()
  return { type: 'image', attrs: { src, alt: '', width: image.naturalWidth > 640 ? 640 : null } }
}
