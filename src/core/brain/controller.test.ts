import { describe, expect, it, vi } from 'vitest'
import { AgentController } from './controller'
import type { AgentEvent, AgentPort, ConversationState, TurnSnapshot } from '../../platform/contracts'

function turn(requestId: string, patch: Partial<TurnSnapshot> = {}): TurnSnapshot {
  return { requestId, sessionId: 'session-a', prompt: '你好', sequence: 0, assistant: '', toolCalls: 0, terminal: null, result: null, error: null, ...patch }
}
function setup(initial: Partial<ConversationState> = {}) {
  let listener: (event: AgentEvent) => void = () => {}
  const snapshot: ConversationState = { cursor: 0, revision: 0, sessionId: 'session-a', active: null, messages: [], ...initial }
  const unlisten = vi.fn()
  const port: AgentPort = {
    listen: vi.fn(async callback => { listener = callback; return unlisten }),
    snapshot: vi.fn(async () => structuredClone(snapshot)),
    ask: vi.fn(async () => {}), cancel: vi.fn(async () => {}),
  }
  const controller = new AgentController(port)
  return { controller, port, snapshot, unlisten, emit: (event: AgentEvent) => listener(event) }
}
const flush = () => new Promise(resolve => setTimeout(resolve, 0))

describe('conversation lifecycle', () => {
  it('does not accept a second prompt just because the command acknowledged the first', async () => {
    const { controller, port, emit } = setup()
    await controller.connect()
    await controller.send('first')
    expect(controller.getSnapshot().busy).toBe(true)
    await controller.send('second')
    expect(port.ask).toHaveBeenCalledTimes(1)
    const id = controller.getSnapshot().requestId!
    expect(port.ask).toHaveBeenCalledWith('first', id, 0)
    emit({ cursor: 1, revision: 0, type: 'finished', exitCode: 0, cancelled: false,
      turn: turn(id, { sequence: 1, assistant: 'reply', result: { status: 'finished' } }) })
    expect(controller.getSnapshot().busy).toBe(false)
    expect(controller.getSnapshot().messages.map(message => message.text)).toEqual(['first', 'reply'])
    await controller.send('second')
    expect(port.ask).toHaveBeenCalledTimes(2)
  })

  it('filters foreign requests, duplicate deltas and late events after conversation changes', async () => {
    const { controller, emit, snapshot } = setup()
    await controller.connect(); await controller.send('first')
    const id = controller.getSnapshot().requestId!
    const delta: AgentEvent = { cursor: 1, revision: 0, type: 'envelope', requestId: id, sequence: 1,
      envelope: { item: { kind: 'assistant.delta', payload: { text: 'hello' } } } }
    emit(delta); emit({ ...delta, cursor: 2 }); emit({ ...delta, cursor: 3, requestId: 'foreign', sequence: 2 })
    expect(controller.getSnapshot().messages.at(-1)?.text).toBe('hello')
    Object.assign(snapshot, { cursor: 4, revision: 1, sessionId: 'session-b', messages: [{ role: 'assistant', text: 'history B' }] })
    emit({ cursor: 4, revision: 1, type: 'conversation', sessionId: 'session-b' })
    emit({ ...delta, cursor: 5, sequence: 3 })
    await flush()
    expect(controller.getSnapshot().messages.map(message => message.text)).toEqual(['history B'])
    expect(controller.getSnapshot().busy).toBe(false)
  })

  it('hydrates a running turn and skips events already included in the snapshot', async () => {
    const active = turn('running', { assistant: 'already streamed', sequence: 3 })
    const { controller, emit, port } = setup({ cursor: 10, active,
      messages: [{ role: 'user', text: 'earlier' }, { role: 'assistant', text: 'earlier reply' }] })
    const base = port.snapshot
    port.snapshot = async () => {
      emit({ cursor: 8, revision: 0, type: 'started', turn: turn('running') })
      emit({ cursor: 9, revision: 0, type: 'envelope', requestId: 'running', sequence: 2,
        envelope: { item: { kind: 'assistant.delta', payload: { text: 'duplicate' } } } })
      return base()
    }
    await controller.connect()
    expect(controller.getSnapshot().messages.map(message => message.text)).toEqual(['earlier', 'earlier reply', '你好', 'already streamed'])
    expect(controller.getSnapshot().busy).toBe(true)
    emit({ cursor: 11, revision: 0, type: 'finished', turn: { ...active, sequence: 4 }, exitCode: 0, cancelled: false })
    expect(controller.getSnapshot().messages.filter(message => message.role === 'assistant')).toHaveLength(2)
  })

  it('keeps newer state when an old command fails after switching', async () => {
    const { controller, port, snapshot, emit } = setup()
    let reject!: (error: Error) => void
    port.ask = () => new Promise((_, fail) => { reject = fail })
    await controller.connect()
    const sending = controller.send('old')
    Object.assign(snapshot, { cursor: 1, revision: 1, sessionId: null })
    emit({ cursor: 1, revision: 1, type: 'conversation', sessionId: null })
    await flush(); reject(new Error('old failure')); await sending
    expect(controller.getSnapshot().messages).toEqual([])
    expect(controller.getSnapshot().error).toBeNull()
  })

  it('shows stderr once, flushes the final response and removes the thinking placeholder', async () => {
    const { controller, emit } = setup()
    await controller.connect(); await controller.send('hi')
    const id = controller.getSnapshot().requestId!
    emit({ cursor: 1, revision: 0, type: 'finished', turn: turn(id, { sequence: 1, error: 'process failed' }), exitCode: 1, cancelled: false })
    expect(controller.getSnapshot().messages.map(message => message.text)).toEqual(['hi', 'process failed'])
    expect(controller.getSnapshot().pose.state).toBe('failed')
  })

  it('releases a listener that resolves after unmount', async () => {
    const { controller, port, unlisten } = setup()
    let resolve!: (cleanup: () => void) => void
    port.listen = () => new Promise(done => { resolve = done })
    const connecting = controller.connect(); controller.disconnect(); resolve(unlisten); await connecting
    expect(unlisten).toHaveBeenCalledOnce()
    expect(port.snapshot).not.toHaveBeenCalled()
  })

  it('restores the failed pose when mounting after the error event', async () => {
    const { controller } = setup({ active: turn('failed', { terminal: 'failed', error: 'API error' }) })
    await controller.connect()
    expect(controller.getSnapshot().pose.state).toBe('failed')
    expect(controller.getSnapshot().messages.filter(message => message.text === 'API error')).toHaveLength(1)
  })

  it('does not show an old cancellation error in a newly selected conversation', async () => {
    const { controller, port, emit, snapshot } = setup()
    let reject!: (error: Error) => void
    port.cancel = () => new Promise((_, fail) => { reject = fail })
    await controller.connect(); await controller.send('old')
    const cancelling = controller.cancel()
    Object.assign(snapshot, { cursor: 1, revision: 1, sessionId: null })
    emit({ cursor: 1, revision: 1, type: 'conversation', sessionId: null })
    await flush(); reject(new Error('old cancellation error')); await cancelling
    expect(controller.getSnapshot().error).toBeNull()
  })
})
