import { useCallback, useEffect, useRef, useState } from 'react'
import { command, errorMessage, isDesktop, listenPetDrop } from '../../platform/desktop'
import { decodeBrowserDrop, decodePayload, type LoadedPet } from './assets'

export function usePetAssets(notify: (text: string) => void, adopted: () => void) {
  const [asset, setAsset] = useState<LoadedPet | null>(null)
  const [dropping, setDropping] = useState(false)
  const current = useRef<LoadedPet | null>(null)
  const generation = useRef(0)
  const replace = useCallback(async (load: () => Promise<LoadedPet>, dir?: string) => {
    const token = ++generation.current
    try {
      const next = await load()
      if (token !== generation.current) { URL.revokeObjectURL(next.url); return false }
      const previous = current.current
      current.current = next
      setAsset(next)
      if (dir) localStorage.setItem('pet-dir', dir)
      if (previous) { URL.revokeObjectURL(previous.url); adopted() }
      if (next.warnings.length) console.info(`[pet] ${next.pet.id}`, next.warnings)
      return true
    } catch (error) { if (token === generation.current) notify(errorMessage(error)); return false }
  }, [notify, adopted])

  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined
    let bootToken = generation.current
    const fromPath = (dir: string) => replace(async () => decodePayload(await command('load_pet', { dir })), dir)
    const boot = async () => {
      const tryCandidate = async (dir: string) => {
        if (disposed || generation.current !== bootToken) return true
        bootToken++
        const loaded = await fromPath(dir)
        return loaded || disposed || generation.current !== bootToken
      }
      const remembered = localStorage.getItem('pet-dir')
      if (remembered && await tryCandidate(remembered)) return
      if (disposed || generation.current !== bootToken) return
      const pets = await command('list_pets')
      for (const pet of pets) {
        if (await tryCandidate(pet.dir)) return
      }
    }
    if (isDesktop()) {
      void boot().catch(error => { if (!disposed) notify(errorMessage(error)) })
      void listenPetDrop(drop => {
        setDropping(drop.type === 'enter' || drop.type === 'over')
        if (drop.type === 'drop' && drop.paths?.[0]) void fromPath(drop.paths[0])
      }).then(cleanup => { if (disposed) cleanup(); else unlisten = cleanup })
        .catch(error => { if (!disposed) notify(errorMessage(error)) })
    } else {
      const over = (event: DragEvent) => { event.preventDefault(); setDropping(true) }
      const leave = () => setDropping(false)
      const drop = (event: DragEvent) => {
        event.preventDefault(); setDropping(false)
        if (event.dataTransfer) { const transfer = event.dataTransfer; void replace(() => decodeBrowserDrop(transfer)) }
      }
      document.addEventListener('dragover', over); document.addEventListener('dragleave', leave); document.addEventListener('drop', drop)
      unlisten = () => { document.removeEventListener('dragover', over); document.removeEventListener('dragleave', leave); document.removeEventListener('drop', drop) }
    }
    return () => {
      disposed = true; generation.current++; unlisten?.()
      if (current.current) { URL.revokeObjectURL(current.current.url); current.current = null }
    }
  }, [replace, notify])
  return { asset, dropping }
}
