import { expect, it } from 'vitest'
import { createAnimator } from './animator'

it('returns to the active brain state after a greeting and a drag', () => {
  let now = 0
  const animator = createAnimator(() => now)
  animator.apply({ kind: 'state', state: 'running' })
  animator.once('waving'); now = 1500; animator.tick()
  expect(animator.machine.name).toBe('running')
  animator.drag('running-left'); animator.apply({ kind: 'state', state: 'jumping' })
  expect(animator.machine.name).toBe('running-left')
  animator.endDrag()
  expect(animator.machine.name).toBe('idle')
})
