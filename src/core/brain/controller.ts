import type { AgentEvent, AgentPort, ConversationState, TurnSnapshot } from '../../platform/contracts'
import { applyEnvelope, createSession, finishTurn } from './reducer'
import { parseEnvelope, parseResult, type BrainEffect, type BrainSession } from './types'

export interface ChatMessage { id: string; role: 'user' | 'assistant' | 'note'; text: string }
type Pose = Extract<BrainEffect, { kind: 'state' }>
export interface AgentView {
  ready: boolean
  busy: boolean
  revision: number
  sessionId: string | null
  requestId: string | null
  messages: ChatMessage[]
  pose: Pose
  error: string | null
}

const messageError = (error: unknown) => error instanceof Error ? error.message : String(error)

// One controller per WebView. The Rust snapshot is authoritative across windows.
export class AgentController {
  private view: AgentView = { ready: false, busy: false, revision: -1, sessionId: null, requestId: null,
    messages: [], pose: { kind: 'state', state: 'idle' }, error: null }
  private listeners = new Set<() => void>()
  private session = createSession()
  private sequence = -1
  private history: ChatMessage[] = []
  private live: ChatMessage[] = []
  private epoch = 0
  private unlisten: (() => void) | null = null
  private noteId = 0

  constructor(private port: AgentPort, private withHistory = true) {}
  getSnapshot = () => this.view
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener) } }
  private update(patch: Partial<AgentView> = {}) {
    this.view = { ...this.view, ...patch, messages: [...this.history, ...this.live] }
    this.listeners.forEach(listener => listener())
  }

  private buffering = false
  private pending: AgentEvent[] = []
  private cursor = -1

  private receive(event: AgentEvent) {
    if (this.buffering) this.pending.push(event)
    else this.accept(event)
  }

  async connect() {
    const epoch = ++this.epoch
    this.buffering = true
    this.pending = []
    try {
      const unlisten = await this.port.listen(event => {
        if (epoch === this.epoch) this.receive(event)
      })
      if (epoch !== this.epoch) { unlisten(); return }
      this.unlisten = unlisten
      await this.synchronize(epoch)
    } catch (error) {
      if (epoch === this.epoch) this.update({ ready: false, error: messageError(error) })
    }
  }

  disconnect() { this.epoch++; this.unlisten?.(); this.unlisten = null; this.pending = [] }

  private async synchronize(epoch: number) {
    this.buffering = true
    try {
      const snapshot = await this.port.snapshot()
      if (epoch !== this.epoch) return
      this.cursor = snapshot.cursor
      this.restore(snapshot)
      const events = this.pending
      this.pending = []
      this.buffering = false
      for (const event of events) this.receive(event)
    } catch (error) {
      if (epoch === this.epoch) {
        this.buffering = false
        this.update({ ready: false, error: messageError(error) })
      }
    }
  }

  private restore(snapshot: ConversationState) {
    this.history = this.withHistory ? snapshot.messages.map((message, i) => ({ ...message, id: `history-${snapshot.revision}-${i}` })) : []
    this.live = []
    this.sequence = -1
    this.session = createSession()
    this.update({ revision: snapshot.revision, sessionId: snapshot.sessionId, requestId: null,
      busy: false, ready: true, error: null, pose: { kind: 'state', state: snapshot.revision > 0 ? 'waving' : 'idle' } })
    if (snapshot.active) this.begin(snapshot.active)
  }

  private hydrate(turn: TurnSnapshot): BrainSession {
    return { ...createSession(), assistant: turn.assistant, toolCalls: turn.toolCalls,
      terminal: turn.terminal, result: turn.result ? parseResult(turn.result) : null,
      spokeError: this.session.spokeError }
  }

  private begin(turn: TurnSnapshot) {
    if (this.view.requestId === turn.requestId) return
    this.session = createSession()
    this.session = this.hydrate(turn)
    this.sequence = turn.sequence
    if (this.withHistory) {
      this.live.push({ id: `${turn.requestId}-user`, role: 'user', text: turn.prompt })
      this.live.push({ id: `${turn.requestId}-thinking`, role: 'note', text: '思考中…' })
    }
    this.update({ requestId: turn.requestId, busy: true, pose: { kind: 'state', state: 'running' }, error: null })
    if (turn.assistant) this.effects([{ kind: 'say', text: turn.assistant, busy: true, messageKind: 'assistant' }])
    if (turn.error) {
      this.effects([{ kind: 'say', text: turn.error, busy: true, messageKind: 'status' }])
      if (turn.terminal === 'failed') this.session.spokeError = true
    }
    if (turn.terminal === 'failed') this.effects([{ kind: 'state', state: 'failed' }])
  }

  private effects(effects: BrainEffect[]) {
    let changed = false
    for (const effect of effects) {
      if (effect.kind === 'state') {
        if (this.view.pose.state !== effect.state || this.view.pose.transient !== effect.transient) {
          this.view = { ...this.view, pose: effect }; changed = true
        }
        continue
      }
      if (!this.withHistory) continue
      if (effect.messageKind === 'assistant') {
        this.live = this.live.filter(message => message.id !== `${this.view.requestId}-thinking`)
        const id = `${this.view.requestId}-assistant`
        const index = this.live.findIndex(message => message.id === id)
        if (index < 0) this.live.push({ id, role: 'assistant', text: effect.text })
        else this.live[index] = { id, role: 'assistant', text: effect.text }
      } else if (effect.text.trim() && this.live.at(-1)?.text !== effect.text.trim()) {
        this.live.push({ id: `note-${++this.noteId}`, role: 'note', text: effect.text.trim() })
      }
      changed = true
    }
    if (changed) this.update()
  }

  accept(event: AgentEvent) {
    if (event.cursor <= this.cursor || event.revision < this.view.revision) return
    this.cursor = event.cursor
    if (event.type === 'conversation') {
      if (event.revision === this.view.revision) return
      this.history = []; this.live = []; this.session = createSession(); this.sequence = -1
      this.update({ revision: event.revision, sessionId: event.sessionId, requestId: null, busy: false,
        ready: false, error: null, pose: { kind: 'state', state: 'waving' } })
      void this.synchronize(this.epoch)
      return
    }
    if (event.revision !== this.view.revision) return
    if (event.type === 'started') { this.begin(event.turn); return }
    const requestId = event.type === 'finished' ? event.turn.requestId : event.requestId
    const sequence = event.type === 'finished' ? event.turn.sequence : event.sequence
    if (requestId !== this.view.requestId || sequence <= this.sequence) return
    this.sequence = sequence
    if (event.type === 'envelope') {
      const envelope = parseEnvelope(event.envelope)
      if (envelope) this.effects(applyEnvelope(this.session, envelope))
    } else {
      this.session = this.hydrate(event.turn)
      if (event.cancelled) { this.session.terminal = 'cancelled'; this.session.result = null }
      if (event.turn.error && !this.session.spokeError && (event.exitCode !== 0 || this.session.terminal === 'failed')) {
        this.session.result = { ...this.session.result, status: 'error', error: event.turn.error }
      }
      this.live = this.live.filter(message => message.id !== `${requestId}-thinking`)
      this.effects(finishTurn(this.session, { exitCode: event.exitCode }))
      if (event.turn.error && event.exitCode === 0 && this.session.terminal !== 'failed' && !['error', 'timeout'].includes(this.session.result?.status ?? '')) {
        this.effects([{ kind: 'say', text: event.turn.error, busy: false, messageKind: 'status' }])
      }
      this.update({ busy: false, sessionId: event.turn.sessionId })
    }
  }

  async send(prompt: string) {
    if (!prompt.trim() || !this.view.ready || this.view.busy) return false
    const revision = this.view.revision
    const requestId = Array.from(crypto.getRandomValues(new Uint8Array(16)), value => value.toString(16).padStart(2, '0')).join('')
    this.begin({ requestId, sessionId: this.view.sessionId, prompt: prompt.trim(), sequence: 0,
      assistant: '', toolCalls: 0, terminal: null, result: null, error: null })
    try { await this.port.ask(prompt.trim(), requestId, revision) }
    catch (error) {
      if (revision === this.view.revision && requestId === this.view.requestId && this.view.busy) {
        this.live = this.live.filter(message => message.id !== `${requestId}-thinking`)
        this.effects([{ kind: 'say', text: `连不上 deep-code：${messageError(error)}`, busy: false, messageKind: 'status' },
          { kind: 'state', state: 'failed' }])
        this.update({ busy: false })
      }
    }
    return true
  }

  async cancel() {
    const { busy, requestId, revision } = this.view
    if (busy && requestId) {
      try { await this.port.cancel(requestId) }
      catch (error) {
        if (revision === this.view.revision && requestId === this.view.requestId) this.update({ error: messageError(error) })
      }
    }
  }
}
