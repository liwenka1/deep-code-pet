import { createStateMachine } from './state-machine'
import { ONE_SHOT, type PetState } from './model'
import type { BrainEffect } from '../brain/types'

export function createAnimator(now = () => Date.now()) {
  const machine = createStateMachine(now)
  let base: PetState = 'idle'
  let dragging = false
  const rest = () => ONE_SHOT.has(base) ? 'idle' : base
  return {
    machine,
    apply(effect: Extract<BrainEffect, { kind: 'state' }>) {
      base = effect.state
      if (!dragging) machine.set(base, effect.transient ? { autoReturn: true } : {})
    },
    once(state: PetState) { if (!dragging) machine.set(state, { then: rest(), autoReturn: true, force: true }) },
    drag(direction: 'running-left' | 'running-right') { dragging = true; machine.set(direction) },
    endDrag() { dragging = false; machine.set(rest()) },
    tick() { if (machine.tick() && ONE_SHOT.has(base) && machine.name === 'idle') base = 'idle' },
  }
}
