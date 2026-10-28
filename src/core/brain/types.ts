import type { PetState } from '../pet/model'

export interface PrintResult {
  status?: string
  result?: string
  error?: string
  session_id?: string
  denied_approvals?: number
}

export interface BrainEnvelope {
  item: { kind: string; payload: Record<string, unknown> }
}

export interface BrainSession {
  assistant: string
  reasoning: string
  toolCalls: number
  lastBubbleAt: number
  terminal: 'completed' | 'cancelled' | 'failed' | null
  result: PrintResult | null
  spokeError: boolean
}

export type BrainEffect =
  | { kind: 'state'; state: PetState; transient?: boolean }
  | { kind: 'say'; text: string; busy: boolean; messageKind: 'assistant' | 'status'; status?: string }

export function parseEnvelope(input: unknown): BrainEnvelope | null {
  let value: unknown = input
  if (typeof value === 'string') {
    try { value = JSON.parse(value) } catch { return null }
  }
  if (!value || typeof value !== 'object') return null
  const raw = value as Record<string, unknown>
  const item = (raw.item ?? raw) as Record<string, unknown> | null
  if (!item || typeof item !== 'object' || typeof item.kind !== 'string') return null
  const payload = item.payload
  if (payload != null && (typeof payload !== 'object' || Array.isArray(payload))) return null
  return { item: { kind: item.kind, payload: (payload ?? {}) as Record<string, unknown> } }
}

export function parseResult(payload: Record<string, unknown>): PrintResult {
  const result: PrintResult = {}
  for (const key of ['status', 'result', 'error', 'session_id'] as const) {
    if (typeof payload[key] === 'string') result[key] = payload[key]
  }
  if (typeof payload.denied_approvals === 'number') result.denied_approvals = payload.denied_approvals
  return result
}

export const payloadText = (payload: Record<string, unknown>, key: string) =>
  typeof payload[key] === 'string' ? payload[key] : ''
