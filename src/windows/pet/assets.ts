import { parsePetJson, validateAtlas } from '../../core/pet/loader'
import type { PetPayload } from '../../platform/contracts'

export interface LoadedPet {
  pet: ReturnType<typeof parsePetJson>['pet']
  url: string
  width: number
  height: number
  warnings: string[]
}

export async function decodePet(json: string, url: string): Promise<LoadedPet> {
  try {
    const { pet, warnings } = parsePetJson(json)
    const image = new Image()
    await new Promise<void>((resolve, reject) => {
      image.onload = () => resolve()
      image.onerror = () => reject(new Error('精灵图解码失败（请使用 webp 或 png）'))
      image.src = url
    })
    const report = validateAtlas({ width: image.naturalWidth, height: image.naturalHeight }, pet.version)
    return { pet, url, width: image.naturalWidth, height: image.naturalHeight, warnings: [...warnings, ...report.warnings] }
  } catch (error) { URL.revokeObjectURL(url); throw error }
}

export function decodePayload(payload: PetPayload) {
  const bytes = Uint8Array.from(atob(payload.image_base64), character => character.charCodeAt(0))
  return decodePet(payload.json, URL.createObjectURL(new Blob([bytes], { type: payload.mime })))
}

async function readDirectory(entry: FileSystemEntry, prefix = ''): Promise<Map<string, File>> {
  if (entry.isFile) {
    const file = await new Promise<File>((resolve, reject) => (entry as FileSystemFileEntry).file(resolve, reject))
    return new Map([[prefix + file.name, file]])
  }
  const out = new Map<string, File>()
  const reader = (entry as FileSystemDirectoryEntry).createReader()
  for (;;) {
    const batch = await new Promise<FileSystemEntry[]>((resolve, reject) => reader.readEntries(resolve, reject))
    if (batch.length === 0) return out
    for (const child of batch) {
      const files = await readDirectory(child, child.isDirectory ? `${prefix}${child.name}/` : prefix)
      files.forEach((file, key) => out.set(key, file))
    }
  }
}

export async function decodeBrowserDrop(transfer: DataTransfer) {
  const item = [...transfer.items].find(item => item.kind === 'file')
  const entry = item?.webkitGetAsEntry()
  if (!entry?.isDirectory) throw new Error('请拖入整只宠物文件夹')
  const files = await readDirectory(entry)
  const jsonFile = files.get('pet.json')
  if (!jsonFile) throw new Error('这个文件夹里没有 pet.json')
  const json = await jsonFile.text()
  const { pet } = parsePetJson(json)
  const sheet = files.get(pet.spritesheetPath.replaceAll('\\', '/').replace(/^\.\//, ''))
  if (!sheet) throw new Error(`文件夹里没有 ${pet.spritesheetPath}`)
  return decodePet(json, URL.createObjectURL(sheet))
}
