import { useEffect, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { command, errorMessage, isDesktop, listenAgent } from '../../platform/desktop'
import { WindowFrame } from '../../shared/WindowFrame'
import './history.css'

function relative(updatedAt: number) {
  const minutes = Math.max(0, Math.floor((Date.now() - updatedAt) / 60000))
  if (minutes < 1) return '刚刚'
  if (minutes < 60) return `${minutes} 分钟前`
  if (minutes < 1440) return `${Math.floor(minutes / 60)} 小时前`
  return `${Math.floor(minutes / 1440)} 天前`
}

export function HistoryWindow() {
  const client = useQueryClient()
  const [error, setError] = useState<string | null>(null)
  const [pending, setPending] = useState(false)
  const query = useQuery({
    queryKey: ['conversations'], enabled: isDesktop(), networkMode: 'always',
    queryFn: async () => {
      const [conversations, state] = await Promise.all([command('list_conversations', { limit: 50 }), command('conversation_state')])
      return { conversations, currentId: state.sessionId }
    },
  })
  const close = () => { void command('close_history').catch(error => setError(errorMessage(error))) }
  useEffect(() => {
    const refresh = () => { void client.invalidateQueries({ queryKey: ['conversations'] }) }
    let disposed = false
    let unlisten: (() => void) | undefined
    void listenAgent(event => { if (event.type === 'conversation' || event.type === 'finished') refresh() })
      .then(cleanup => { if (disposed) cleanup(); else unlisten = cleanup })
      .catch(error => { if (!disposed) setError(errorMessage(error)) })
    const keydown = (event: KeyboardEvent) => { if (event.key === 'Escape') close() }
    window.addEventListener('focus', refresh)
    document.addEventListener('keydown', keydown)
    return () => { disposed = true; unlisten?.(); window.removeEventListener('focus', refresh); document.removeEventListener('keydown', keydown) }
  }, [client])

  async function select(id?: string) {
    if (pending) return
    setPending(true); setError(null)
    try {
      if (id) await command('open_conversation', { id })
      else await command('new_conversation')
      await command('close_history')
    } catch (error) { setError(errorMessage(error)) }
    finally { setPending(false) }
  }

  const conversations = query.data?.conversations ?? []
  return (
    <WindowFrame title="历史对话" className="history-window" onClose={close}
      actions={<button id="new" type="button" disabled={pending || !isDesktop()} onClick={() => { void select() }}>＋ 新建</button>}>
      {(error || query.error) && <div className="panel-error" role="alert">{error || errorMessage(query.error)}</div>}
      <div id="list" role="listbox" aria-label="历史对话" aria-busy={query.isFetching}>
        {conversations.map(conversation => {
          const current = conversation.id === query.data?.currentId
          return <button type="button" key={conversation.id} className={`row${current ? ' current' : ''}`}
            role="option" aria-selected={current} disabled={pending} onClick={() => { void select(conversation.id) }}>
            <span className="title">{conversation.title}</span>
            {current && <span className="badge">当前</span>}
            <span className="meta">{relative(conversation.updated_at_ms)}</span>
          </button>
        })}
      </div>
      {conversations.length === 0 && <footer id="empty">{!isDesktop() ? '请在桌面应用中查看历史对话' : query.isFetching ? '正在加载…' : query.error ? '暂时无法加载历史对话' : '还没有对话'}</footer>}
    </WindowFrame>
  )
}
