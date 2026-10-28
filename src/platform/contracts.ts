import type { BrainEnvelope } from '../core/brain/types'

export interface HistoryMessage { role: 'user' | 'assistant'; text: string }
export interface Conversation { id: string; title: string; updated_at_ms: number; turns: number }
export interface PetEntry { slug: string; name: string; dir: string }
export interface PetPayload { json: string; image_base64: string; mime: string }
export interface DoctorReport { api_key?: { source?: string }; version?: string; default_model?: string; config_path?: string }
export interface TurnSnapshot {
  requestId: string
  sessionId: string | null
  prompt: string
  sequence: number
  assistant: string
  toolCalls: number
  terminal: 'completed' | 'cancelled' | 'failed' | null
  result: Record<string, unknown> | null
  error: string | null
}
export interface ConversationState { cursor: number; revision: number; sessionId: string | null; active: TurnSnapshot | null; messages: HistoryMessage[] }
export type AgentEvent = { cursor: number } & (
  | { type: 'conversation'; revision: number; sessionId: string | null }
  | { type: 'started'; revision: number; turn: TurnSnapshot }
  | { type: 'envelope'; revision: number; requestId: string; sequence: number; envelope: BrainEnvelope }
  | { type: 'finished'; revision: number; turn: TurnSnapshot; exitCode: number; cancelled: boolean }
)

export interface AgentPort {
  listen(callback: (event: AgentEvent) => void): Promise<() => void>
  snapshot(): Promise<ConversationState>
  ask(prompt: string, requestId: string, expectedRevision: number): Promise<void>
  cancel(requestId: string): Promise<void>
}

export interface Commands {
  conversation_state: { args: undefined; result: ConversationState }
  ask_deepcode: { args: { prompt: string; requestId: string; expectedRevision: number }; result: void }
  cancel_turn: { args: { requestId: string }; result: void }
  load_history: { args: { limit: number; sessionId: string }; result: HistoryMessage[] }
  list_conversations: { args: { limit: number }; result: Conversation[] }
  new_conversation: { args: undefined; result: void }
  open_conversation: { args: { id: string }; result: void }
  open_chat: { args: undefined; result: void }
  close_chat: { args: undefined; result: void }
  close_history: { args: undefined; result: void }
  open_history: { args: undefined; result: void }
  show_pet_menu: { args: undefined; result: void }
  load_pet: { args: { dir: string }; result: PetPayload }
  list_pets: { args: undefined; result: PetEntry[] }
  doctor_summary: { args: undefined; result: DoctorReport }
  get_agent_bin: { args: undefined; result: string | null }
  get_model: { args: undefined; result: string }
  set_model: { args: { model: string }; result: string }
  drag_begin: { args: undefined; result: void }
  drag_update: { args: undefined; result: void }
  drag_end: { args: undefined; result: void }
  set_window_layout: { args: { width: number; height: number; spriteW: number; spriteH: number; pad: number; anchor: string }; result: string }
  save_window_pos: { args: undefined; result: void }
  restore_window_pos: { args: undefined; result: void }
}
