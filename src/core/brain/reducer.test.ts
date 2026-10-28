import { expect, it } from 'vitest'
import { applyEnvelope, createSession, finishTurn } from './reducer'
import { parseEnvelope } from './types'

it('rejects malformed envelopes at the transport boundary', () => {
  for (const input of ['noise', '{', null, { item: null }, { item: { kind: 'assistant.delta', payload: [] } }]) {
    expect(parseEnvelope(input)).toBeNull()
  }
  const session = createSession()
  applyEnvelope(session, { item: { kind: 'assistant.delta', payload: { text: { unexpected: true } } } })
  expect(session.assistant).toBe('')
})

it('treats a nonzero process exit as failure even after a success report', () => {
  const session = createSession()
  session.result = { status: 'finished', result: 'partial' }
  session.terminal = 'completed'
  expect(finishTurn(session, { exitCode: 1 })).toContainEqual({ kind: 'state', state: 'failed' })
})
