import { copyFile, mkdir, readFile, readdir, rm } from 'node:fs/promises'
import { resolve } from 'node:path'

// Every native window must have a physical entry in the embedded asset bundle.
// All entries are the same Start SPA shell; the router selects the window UI.
const client = resolve('dist/client')
const shell = resolve(client, 'index.html')
await readFile(shell, 'utf8')
await Promise.all(['chat.html', 'history.html'].map(name => copyFile(shell, resolve(client, name))))
// Keep Tauri's frontendDist stable and exclude the unused Start server build.
const desktop = resolve('dist/desktop')
await rm(desktop, { recursive: true, force: true })
await mkdir(desktop, { recursive: true })
const { cp } = await import('node:fs/promises')
for (const entry of await readdir(client)) {
  await cp(resolve(client, entry), resolve(desktop, entry), { recursive: true })
}
