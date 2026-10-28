import { useEffect, useRef, type PointerEvent } from 'react'
import type { createAnimator } from '../../core/pet/animator'
import { command, isDesktop } from '../../platform/desktop'

export function usePetDrag(animator: ReturnType<typeof createAnimator>, openChat: () => void) {
  const tap = useRef<ReturnType<typeof setTimeout> | null>(null)
  const drag = useRef({ down: false, moved: false, distance: 0, x: 0, y: 0, frame: 0, begin: Promise.resolve() })
  const cancelTap = () => { if (tap.current) clearTimeout(tap.current); tap.current = null }
  const start = (event: PointerEvent) => {
    if (event.button !== 0) return
    const state = drag.current
    state.down = true; state.moved = false; state.distance = 0; state.x = event.screenX; state.y = event.screenY
    // Keep a first click pending until the second release so double clicks stay distinct.
    if (isDesktop()) state.begin = command('drag_begin').catch(() => {})
    event.currentTarget.setPointerCapture?.(event.pointerId)
  }
  useEffect(() => {
    const move = (event: globalThis.PointerEvent) => {
      const state = drag.current
      if (!state.down) return
      const dx = event.movementX || event.screenX - state.x
      const dy = event.movementY || event.screenY - state.y
      state.x = event.screenX; state.y = event.screenY
      state.distance += Math.abs(dx) + Math.abs(dy)
      if (!state.moved && state.distance < 4) return
      state.moved = true; cancelTap()
      if (dx) animator.drag(dx > 0 ? 'running-right' : 'running-left')
      if (!isDesktop() || state.frame) return
      state.frame = requestAnimationFrame(() => {
        state.frame = 0
        void state.begin.then(() => { if (state.down) return command('drag_update') }).catch(() => {})
      })
    }
    const end = (allowTap: boolean) => {
      const state = drag.current
      if (!state.down) return
      state.down = false
      if (state.frame) cancelAnimationFrame(state.frame)
      state.frame = 0
      const moved = state.moved
      if (isDesktop()) void state.begin.then(async () => {
        if (moved) await command('drag_update')
        await command('drag_end')
        if (moved) await command('save_window_pos')
      }).catch(() => {})
      if (state.moved) {
        animator.endDrag(); state.moved = false
      } else if (allowTap) {
        if (tap.current) { cancelTap(); openChat() }
        else tap.current = setTimeout(() => { tap.current = null; animator.once('jumping') }, 300)
      }
    }
    const up = () => end(true)
    const cancel = () => { cancelTap(); end(false) }
    window.addEventListener('pointermove', move); window.addEventListener('pointerup', up)
    window.addEventListener('pointercancel', cancel); window.addEventListener('blur', cancel)
    return () => {
      window.removeEventListener('pointermove', move); window.removeEventListener('pointerup', up)
      window.removeEventListener('pointercancel', cancel); window.removeEventListener('blur', cancel)
      cancel()
    }
  }, [animator, openChat])
  return { start, cancelTap }
}
