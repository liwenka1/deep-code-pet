import type { ReactNode } from 'react'

export function WindowFrame({ title, className, onClose, actions, children }: {
  title: string; className: string; onClose: () => void; actions?: ReactNode; children: ReactNode
}) {
  return (
    <main className={`window-panel ${className}`}>
      <header id="head" data-tauri-drag-region>
        <span id="title">{title}</span>
        <span id="actions">
          {actions}
          <button id="close" type="button" title="收起 (Esc)" aria-label="收起窗口" onClick={onClose}>✕</button>
        </span>
      </header>
      {children}
    </main>
  )
}
