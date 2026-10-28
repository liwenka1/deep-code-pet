// @vitest-environment jsdom
import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { usePetAssets } from './use-pet-assets'
import type { PetPayload } from '../../platform/contracts'

const mock = vi.hoisted(() => ({ command: vi.fn(), drop: null as null | ((drop: { type: string; paths?: string[] }) => void) }))
vi.mock('../../platform/desktop', () => ({
  isDesktop: () => true, command: mock.command,
  errorMessage: (error: unknown) => error instanceof Error ? error.message : String(error),
  listenPetDrop: async (callback: typeof mock.drop) => { mock.drop = callback; return () => { mock.drop = null } },
}))

class FakeImage {
  static instances: FakeImage[] = []
  naturalWidth = 1536
  naturalHeight = 1872
  onload: (() => void) | null = null
  onerror: (() => void) | null = null
  constructor() { FakeImage.instances.push(this) }
  set src(_value: string) {}
}
const payload = (id: string): PetPayload => ({ json: JSON.stringify({ id, displayName: id, spritesheetPath: 'sheet.png' }), image_base64: '', mime: 'image/png' })
let created = 0
beforeEach(() => {
  localStorage.clear(); created = 0; FakeImage.instances = []; mock.command.mockReset()
  vi.stubGlobal('Image', FakeImage)
  Object.assign(URL, { createObjectURL: vi.fn(() => `blob:pet-${++created}`), revokeObjectURL: vi.fn() })
  mock.command.mockImplementation(async (name: string, args?: { dir: string }) => name === 'list_pets' ? [{ dir: 'first' }] : payload(args!.dir))
})
afterEach(() => vi.unstubAllGlobals())

it('persists only a decoded pet, keeps the previous one on failure and releases its URLs', async () => {
  const notify = vi.fn(), adopted = vi.fn()
  const { result, unmount } = renderHook(() => usePetAssets(notify, adopted))
  await waitFor(() => expect(FakeImage.instances).toHaveLength(1))
  expect(localStorage.getItem('pet-dir')).toBeNull()
  await act(async () => FakeImage.instances[0].onload?.())
  expect(result.current.asset?.pet.id).toBe('first')
  expect(localStorage.getItem('pet-dir')).toBe('first')
  act(() => mock.drop?.({ type: 'drop', paths: ['broken'] }))
  await waitFor(() => expect(FakeImage.instances).toHaveLength(2))
  await act(async () => FakeImage.instances[1].onerror?.())
  expect(result.current.asset?.pet.id).toBe('first')
  expect(localStorage.getItem('pet-dir')).toBe('first')
  expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:pet-2')
  expect(URL.revokeObjectURL).not.toHaveBeenCalledWith('blob:pet-1')
  unmount()
  expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:pet-1')
})

it('does not let startup scanning replace a pet dropped while the scan was pending', async () => {
  let complete!: (value: { dir: string }[]) => void
  mock.command.mockImplementation((name: string, args?: { dir: string }) => name === 'list_pets'
    ? new Promise(resolve => { complete = resolve }) : Promise.resolve(payload(args!.dir)))
  const notify = vi.fn(), adopted = vi.fn()
  const { result, unmount } = renderHook(() => usePetAssets(notify, adopted))
  await waitFor(() => expect(mock.drop).toBeTypeOf('function'))
  act(() => mock.drop?.({ type: 'drop', paths: ['chosen'] }))
  await waitFor(() => expect(FakeImage.instances).toHaveLength(1))
  await act(async () => FakeImage.instances[0].onload?.())
  await act(async () => complete([{ dir: 'startup-default' }]))
  expect(result.current.asset?.pet.id).toBe('chosen')
  expect(FakeImage.instances).toHaveLength(1)
  unmount()
})

it('releases an image that finishes decoding after unmount', async () => {
  const notify = vi.fn(), adopted = vi.fn()
  const { unmount } = renderHook(() => usePetAssets(notify, adopted))
  await waitFor(() => expect(FakeImage.instances).toHaveLength(1))
  unmount()
  await act(async () => FakeImage.instances[0].onload?.())
  expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:pet-1')
  expect(localStorage.getItem('pet-dir')).toBeNull()
})
