import { test, expect, type Page } from '@playwright/test'
import type { AgentEvent, ConversationState, TurnSnapshot } from '../../src/platform/contracts'

type Unstamped<T> = T extends unknown ? Omit<T, 'cursor'> : never

interface Harness {
  calls: { name: string; args: Record<string, unknown> }[]
  finish(text: string): void
  switchConversation(): void
  badPet(): void
}
declare global { interface Window { __petTest: Harness } }

async function desktop(page: Page) {
  await page.addInitScript(() => {
    const callbacks = new Map<number, (value: unknown) => void>()
    const listeners = new Map<number, { event: string; callback: number }>()
    const pending = new Map<string, () => void>()
    let nextId = 1
    const state: ConversationState = { cursor: 0, revision: 0, sessionId: 'session-a', active: null,
      messages: [{ role: 'user', text: '之前的问题' }, { role: 'assistant', text: '之前的回复' }] }
    const calls: Harness['calls'] = []
    function emit(name: string, payload: unknown) {
      for (const [id, listener] of listeners) {
        if (listener.event === name) callbacks.get(listener.callback)?.({ event: name, id, payload })
      }
    }
    function agent(event: Unstamped<AgentEvent>) { emit('agent-event', { ...event, cursor: ++state.cursor }) }
    function finish(text: string, cancelled = false) {
      const active = state.active
      if (!active) return
      active.assistant = text
      active.sequence++
      agent({ type: 'envelope', revision: state.revision, requestId: active.requestId, sequence: active.sequence,
        envelope: { item: { kind: 'assistant.delta', payload: { text } } } })
      active.sequence++
      active.terminal = cancelled ? 'cancelled' : 'completed'
      active.result = cancelled ? null : { status: 'finished', result: text, session_id: state.sessionId }
      state.messages.push({ role: 'user', text: active.prompt })
      if (text) state.messages.push({ role: 'assistant', text })
      state.active = null
      agent({ type: 'finished', revision: state.revision, turn: active, exitCode: 0, cancelled })
      pending.get(active.requestId)?.(); pending.delete(active.requestId)
    }
    function select(sessionId: string | null) {
      const old = state.active
      state.active = null; state.revision++; state.sessionId = sessionId
      state.messages = sessionId ? [{ role: 'assistant', text: '另一条历史' }] : []
      agent({ type: 'conversation', revision: state.revision, sessionId })
      if (old) {
        agent({ type: 'finished', revision: state.revision - 1, turn: { ...old, sequence: old.sequence + 1, assistant: '过期回复' }, exitCode: 0, cancelled: false })
        pending.get(old.requestId)?.(); pending.delete(old.requestId)
      }
    }
    Object.assign(window, {
      isTauri: true,
      __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: (_event: string, id: number) => listeners.delete(id) },
      __TAURI_INTERNALS__: {
        metadata: { currentWindow: { label: 'test' }, currentWebview: { label: 'test' } },
        transformCallback(callback: (value: unknown) => void) { const id = nextId++; callbacks.set(id, callback); return id },
        async invoke(name: string, args: Record<string, unknown> = {}) {
          calls.push({ name, args })
          if (name === 'plugin:event|listen') { const id = nextId++; listeners.set(id, { event: String(args.event), callback: Number(args.handler) }); return id }
          if (name === 'plugin:event|unlisten') { listeners.delete(Number(args.eventId)); return }
          if (name === 'conversation_state') return structuredClone(state)
          if (name === 'ask_deepcode') {
            if (args.expectedRevision !== state.revision) throw new Error('对话已切换，请重新发送')
            if (state.active) throw new Error('上一轮还没结束')
            const active: TurnSnapshot = { requestId: String(args.requestId), sessionId: state.sessionId, prompt: String(args.prompt), sequence: 0,
              assistant: '', toolCalls: 0, terminal: null, result: null, error: null }
            state.active = active
            agent({ type: 'started', revision: state.revision, turn: active })
            return new Promise<void>(resolve => pending.set(active.requestId, resolve))
          }
          if (name === 'cancel_turn') { finish('', true); return }
          if (name === 'new_conversation') { select(null); return }
          if (name === 'open_conversation') { select(String(args.id)); return }
          if (name === 'list_conversations') return [
            { id: 'session-a', title: '之前的问题', updated_at_ms: Date.now(), turns: 1 },
            { id: 'session-b', title: '另一条历史', updated_at_ms: Date.now() - 60000, turns: 1 },
          ]
          if (name === 'doctor_summary') return { api_key: { source: 'file' } }
          if (name === 'set_window_layout') return 'br'
          if (name === 'list_pets') return [{ dir: 'fixture-pet', slug: 'fixture', name: '测试宠物' }]
          if (name === 'load_pet') {
            if (args.dir === 'invalid-pet') return { json: '{}', image_base64: '', mime: 'image/png' }
            const canvas = document.createElement('canvas'); canvas.width = 1536; canvas.height = 1872
            const context = canvas.getContext('2d')!; context.fillStyle = '#e893aa'; context.fillRect(24, 24, 144, 172)
            return { json: JSON.stringify({ id: 'fixture', displayName: '测试宠物', spritesheetPath: 'sheet.png' }), image_base64: canvas.toDataURL('image/png').split(',')[1], mime: 'image/png' }
          }
        },
      },
      __petTest: { calls, finish, switchConversation: () => select('session-b'), badPet: () => emit('tauri://drag-drop', { paths: ['invalid-pet'], position: { x: 0, y: 0 } }) },
    })
  })
}

test('all three physical SPA entries boot without hydration or script errors', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', error => errors.push(`${page.url()}: ${error.message}`))
  for (const [path, selector] of [['/', '.pet-window'], ['/chat.html', '.chat-window'], ['/history.html', '.history-window']]) {
    await page.goto(path)
    await expect(page.locator(selector)).toBeVisible()
    await expect(page.locator('body')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)')
  }
  expect(errors).toEqual([])
})

test('chat keeps a turn busy, preserves the draft and flushes the final reply', async ({ page }) => {
  await desktop(page); await page.goto('/chat.html')
  const input = page.getByRole('textbox', { name: '消息' })
  await expect(page.getByText('之前的回复', { exact: true })).toBeVisible()
  await input.fill('第一轮'); await input.press('Enter')
  await expect(page.getByRole('button', { name: '停止回复' })).toBeVisible()
  await input.fill('第二轮草稿'); await input.press('Enter')
  expect(await page.evaluate(() => window.__petTest.calls.filter(call => call.name === 'ask_deepcode').length)).toBe(1)
  await expect(input).toHaveValue('第二轮草稿')
  await page.evaluate(() => window.__petTest.finish('完整回复'))
  await expect(page.getByText('完整回复', { exact: true })).toBeVisible()
  await expect(page.getByText('思考中…', { exact: true })).toHaveCount(0)
  await expect(page.getByRole('button', { name: '发送' })).toBeEnabled()
})

test('switching conversations drops old stream events', async ({ page }) => {
  await desktop(page); await page.goto('/chat.html')
  const input = page.getByRole('textbox', { name: '消息' })
  await input.fill('旧问题'); await input.press('Enter')
  await expect(page.getByRole('button', { name: '停止回复' })).toBeVisible()
  await page.evaluate(() => window.__petTest.switchConversation())
  await expect(page.getByText('另一条历史', { exact: true })).toBeVisible()
  await expect(page.getByText('过期回复', { exact: true })).toHaveCount(0)
  await expect(input).toBeEnabled()
  await input.fill('新会话的问题'); await input.press('Enter')
  await expect(page.getByRole('button', { name: '停止回复' })).toBeVisible()
  expect(await page.evaluate(() => window.__petTest.calls.filter(call => call.name === 'ask_deepcode').at(-1)?.args.expectedRevision)).toBe(1)
})

test('history selects conversations and closes its own native window', async ({ page }) => {
  await desktop(page); await page.goto('/history.html')
  await expect(page.getByRole('option', { name: /之前的问题/ })).toHaveAttribute('aria-selected', 'true')
  await page.getByRole('option', { name: /另一条历史/ }).click()
  await expect.poll(() => page.evaluate(() => window.__petTest.calls.filter(call => call.name === 'close_history').length)).toBe(1)
})

test('pet validates replacement assets and keeps the last working pet on failure', async ({ page }) => {
  await desktop(page); await page.goto('/')
  const sprite = page.getByRole('img', { name: '测试宠物' })
  await expect(sprite).toBeVisible()
  await page.evaluate(() => window.__petTest.badPet())
  await expect(page.getByRole('status')).toContainText('缺字段')
  await expect(sprite).toBeVisible()
  expect(await page.evaluate(() => localStorage.getItem('pet-dir'))).toBe('fixture-pet')
})

test('chat preserves multiline and IME input and can stop an active reply', async ({ page }) => {
  await desktop(page); await page.goto('/chat.html')
  const input = page.getByRole('textbox', { name: '消息' })
  await input.fill('多行消息'); await input.press('Shift+Enter')
  await expect(input).toHaveValue('多行消息\n')
  await input.dispatchEvent('keydown', { key: 'Enter', code: 'Enter', isComposing: true })
  expect(await page.evaluate(() => window.__petTest.calls.filter(call => call.name === 'ask_deepcode').length)).toBe(0)
  await input.press('Enter')
  await page.getByRole('button', { name: '停止回复' }).click()
  await expect(page.getByText('好，停下了', { exact: true })).toBeVisible()
  await expect(input).toBeEnabled()
})

test('pet double-click and scale shortcuts keep working', async ({ page }) => {
  await desktop(page); await page.goto('/')
  const sprite = page.getByRole('img', { name: '测试宠物' })
  await expect(sprite).toBeVisible()
  await sprite.dblclick()
  await expect.poll(() => page.evaluate(() => window.__petTest.calls.filter(call => call.name === 'open_chat').length)).toBe(1)
  await page.keyboard.press('Control+-')
  await expect(sprite).toHaveCSS('width', '115px')
  expect(await page.evaluate(() => localStorage.getItem('pet-scale-v2'))).toBe('0.6')
})
