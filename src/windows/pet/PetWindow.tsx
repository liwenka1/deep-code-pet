import { useCallback, useEffect, useRef, useState, type CSSProperties } from 'react'
import { createAnimator } from '../../core/pet/animator'
import { FRAME_H, FRAME_W, frameRect } from '../../core/pet/model'
import { command, errorMessage, isDesktop } from '../../platform/desktop'
import { useAgent } from '../../shared/use-agent'
import { usePetAssets } from './use-pet-assets'
import { usePetDrag } from './use-pet-drag'
import './pet.css'

const clampScale = (scale: number) => Math.min(1.6, Math.max(.6, Math.round(scale * 100) / 100))

export function PetWindow() {
  const [animator] = useState(() => createAnimator())
  const { view } = useAgent(false)
  const [notice, setNotice] = useState<string | null>(null)
  const [scale, setScale] = useState(.7)
  const [anchor, setAnchor] = useState('br')
  const sprite = useRef<HTMLDivElement>(null)
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  const anchorRef = useRef('br')
  const layout = useRef(Promise.resolve())
  const restored = useRef(false)
  const notify = useCallback((text: string) => {
    setNotice(text.length > 120 ? `${text.slice(0, 120)}…` : text)
    clearTimeout(timer.current)
    timer.current = setTimeout(() => setNotice(null), 4000)
  }, [])
  const wave = useCallback(() => animator.once('waving'), [animator])
  const { asset, dropping } = usePetAssets(notify, wave)
  const openChat = useCallback(() => {
    wave()
    if (isDesktop()) void command('open_chat').catch(error => notify(errorMessage(error)))
  }, [wave, notify])
  const { start, cancelTap } = usePetDrag(animator, openChat)

  useEffect(() => { animator.apply(view.pose) }, [animator, view.pose])
  useEffect(() => {
    const stored = Number(localStorage.getItem('pet-scale-v2'))
    if (Number.isFinite(stored) && stored > 0) setScale(clampScale(stored))
    const hello = setTimeout(wave, 400)
    return () => { clearTimeout(hello); clearTimeout(timer.current) }
  }, [wave])
  useEffect(() => {
    if (!isDesktop()) return
    let disposed = false
    layout.current = layout.current.then(async () => {
      if (disposed) return
      const next = await command('set_window_layout', {
        width: Math.max(140, Math.round(FRAME_W * scale + 16)), height: Math.max(160, Math.round(FRAME_H * scale + 16)),
        spriteW: FRAME_W * scale, spriteH: FRAME_H * scale, pad: 8, anchor: anchorRef.current,
      })
      anchorRef.current = next
      if (!disposed) setAnchor(next)
      if (!restored.current) { await command('restore_window_pos'); restored.current = true }
    }).catch(error => { if (!disposed) notify(errorMessage(error)) })
    return () => { disposed = true }
  }, [scale, notify])
  useEffect(() => {
    const node = sprite.current
    if (!asset || !node) return
    let frame = 0
    let last = ''
    function draw() {
      if (!node || !asset) return
      animator.tick()
      const index = animator.machine.frameIndex()
      const key = `${animator.machine.name}:${index}`
      if (key !== last) {
        last = key
        const rect = frameRect(animator.machine.name, index, asset.pet.version)
        node.style.backgroundPosition = `${-rect.x * scale}px ${-rect.y * scale}px`
        node.dataset.state = animator.machine.name
      }
      frame = requestAnimationFrame(draw)
    }
    draw()
    return () => cancelAnimationFrame(frame)
  }, [asset, scale, animator])
  useEffect(() => {
    let disposed = false
    if (isDesktop()) {
      void command('doctor_summary').then(report => {
        if (!disposed && report.api_key?.source === 'missing') notify('我还没连上大脑：先在终端跑一次 deepcode 把 key 配好')
      }).catch(error => { if (!disposed) notify(errorMessage(error)) })
    }
    const keydown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') { setNotice(null); return }
      if (!(event.metaKey || event.ctrlKey)) return
      if (event.shiftKey && event.key.toLowerCase() === 'r') {
        event.preventDefault()
        void command('new_conversation').catch(error => notify(errorMessage(error)))
      } else if (['-', '_', '=', '+'].includes(event.key)) {
        event.preventDefault()
        setScale(value => { const next = clampScale(value + (event.key === '-' || event.key === '_' ? -.1 : .1)); localStorage.setItem('pet-scale-v2', String(next)); return next })
      }
    }
    document.addEventListener('keydown', keydown)
    return () => { disposed = true; document.removeEventListener('keydown', keydown) }
  }, [notify])

  const style = { '--sprite-w': `${Math.round(FRAME_W * scale)}px`, '--sprite-h': `${Math.round(FRAME_H * scale)}px` } as CSSProperties
  return (
    <main className="pet-window" data-anchor={anchor} style={style}>
      <div id="pet" onPointerDown={start} onContextMenu={event => {
        event.preventDefault(); cancelTap()
        void command('show_pet_menu').catch(error => notify(errorMessage(error)))
      }}>
        <button id="chat-btn" type="button" title="打开对话" aria-label="打开对话"
          onPointerDown={event => event.stopPropagation()} onPointerUp={event => event.stopPropagation()}
          onClick={event => { event.stopPropagation(); cancelTap(); openChat() }}>💬</button>
        <div id="sprite" ref={sprite} className={asset ? '' : 'empty'} role="img" aria-label={asset?.pet.displayName ?? '宠物'}
          style={asset ? { backgroundImage: `url(${asset.url})`, backgroundSize: `${asset.width * scale}px ${asset.height * scale}px` } : undefined} />
      </div>
      <div id="drop" hidden={!dropping}><div className="drop-card">松手换上这只宠物</div></div>
      {(notice || !asset) && <div id="hint" role="status">{notice ?? '拖入宠物开始'}</div>}
    </main>
  )
}
