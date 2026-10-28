import { invoke, isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import type { AgentEvent, AgentPort, Commands } from './contracts'

export const isDesktop = () => typeof window !== 'undefined' && isTauri()
export const errorMessage = (error: unknown) =>
  (error instanceof Error ? error.message : String(error)).replace(/^Error:\s*/, '')

export function command<K extends keyof Commands>(name: K, ...params: Commands[K]['args'] extends undefined ? [] : [Commands[K]['args']]): Promise<Commands[K]['result']> {
  if (!isDesktop()) return Promise.reject(new Error('请在桌面应用中使用此功能'))
  return invoke<Commands[K]['result']>(name, params[0])
}

export function listenAgent(callback: (event: AgentEvent) => void) {
  if (!isDesktop()) return Promise.resolve(() => {})
  return listen<AgentEvent>('agent-event', event => callback(event.payload))
}

export const agentPort: AgentPort = {
  listen: listenAgent,
  snapshot: () => command('conversation_state'),
  ask: (prompt, requestId, expectedRevision) => command('ask_deepcode', { prompt, requestId, expectedRevision }),
  cancel: requestId => command('cancel_turn', { requestId }),
}

export async function listenPetDrop(callback: (drop: { type: string; paths?: string[] }) => void) {
  return getCurrentWebview().onDragDropEvent(event => callback(event.payload))
}
