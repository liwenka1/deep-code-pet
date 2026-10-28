import { useEffect, useRef, useState, type FormEvent } from 'react'
import { command, errorMessage } from '../../platform/desktop'
import { useAgent } from '../../shared/use-agent'
import { WindowFrame } from '../../shared/WindowFrame'
import './chat.css'

export function ChatWindow() {
  const { view, controller } = useAgent()
  const [draft, setDraft] = useState('')
  const [error, setError] = useState<string | null>(null)
  const input = useRef<HTMLTextAreaElement>(null)
  const log = useRef<HTMLDivElement>(null)
  const follow = useRef(true)

  const close = () => { void command('close_chat').catch(error => setError(errorMessage(error))) }
  useEffect(() => {
    const keydown = (event: KeyboardEvent) => { if (event.key === 'Escape') close() }
    const focus = () => { input.current?.focus() }
    document.addEventListener('keydown', keydown)
    window.addEventListener('focus', focus)
    return () => { document.removeEventListener('keydown', keydown); window.removeEventListener('focus', focus) }
  }, [])
  useEffect(() => {
    if (follow.current && log.current) log.current.scrollTop = log.current.scrollHeight
  }, [view.messages])
  useEffect(() => { if (view.ready && document.hasFocus()) input.current?.focus() }, [view.ready, view.busy])
  useEffect(() => {
    if (!input.current) return
    input.current.style.height = 'auto'
    input.current.style.height = `${Math.min(input.current.scrollHeight, 96)}px`
  }, [draft])

  function submit(event: FormEvent) {
    event.preventDefault()
    if (!view.ready || view.busy || !draft.trim()) return
    const text = draft.trim()
    setDraft(''); setError(null); follow.current = true
    void controller.send(text)
  }

  return (
    <WindowFrame title="聊天" className="chat-window" onClose={close}>
      {(view.error || error) && <div className="panel-error" role="alert">{error || view.error}</div>}
      <div id="log" ref={log} role="log" aria-live="polite" aria-busy={view.busy}
        onScroll={() => { const node = log.current; if (node) follow.current = node.scrollHeight - node.scrollTop - node.clientHeight < 40 }}>
        {view.messages.map(message => <div className={`msg ${message.role}`} key={message.id}>{message.text}</div>)}
        {view.messages.length === 0 && <div className="msg note">{view.ready ? '新对话，说点什么吧' : view.error ? '暂时无法加载对话' : '正在加载对话…'}</div>}
      </div>
      <form id="form" onSubmit={submit}>
        <textarea id="input" ref={input} rows={1} value={draft} aria-label="消息"
          disabled={!view.ready} placeholder="说点什么…  Enter 发送 / Shift+Enter 换行"
          onChange={event => setDraft(event.target.value)}
          onKeyDown={event => {
            if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing && event.keyCode !== 229) {
              event.preventDefault()
              if (!view.busy) event.currentTarget.form?.requestSubmit()
            }
          }} />
        {view.busy
          ? <button id="send" type="button" aria-label="停止回复" onClick={() => { void controller.cancel() }}>停止</button>
          : <button id="send" type="submit" disabled={!view.ready || !draft.trim()}>发送</button>}
      </form>
    </WindowFrame>
  )
}
