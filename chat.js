// 聊天窗口（chat）：完整记录 + 多行输入 + 流式渲染。
// 它是独立窗口 —— 打开/收起不改变宠物窗口尺寸，所以宠物不会动。

import { applyEnvelope, createSession, finishTurn } from "./brain.mjs";

const tauri = window.__TAURI__;
const el = {
  log: document.getElementById("log"),
  form: document.getElementById("form"),
  input: document.getElementById("input"),
  send: document.getElementById("send"),
  close: document.getElementById("close"),
};

let session = createSession();
let draftNode = null; // 正在流式写入的那条 assistant 气泡
let thinkingNode = null; // "思考中…" 占位
let lastNoteText = "";

// ── 渲染 ─────────────────────────────────────────────────────────────────
function scrollLog() {
  el.log.scrollTop = el.log.scrollHeight;
}

function appendMessage(role, text) {
  const node = document.createElement("div");
  node.className = `msg ${role}`;
  node.textContent = text;
  el.log.appendChild(node);
  scrollLog();
  return node;
}

function pushNote(text) {
  const trimmed = text.trim();
  if (!trimmed || trimmed === lastNoteText) return; // 连续同一条工具提示不刷屏
  lastNoteText = trimmed;
  appendMessage("note", trimmed);
}

function dropThinking() {
  if (!thinkingNode) return;
  thinkingNode.remove();
  thinkingNode = null;
}

/** 流式增量：brain 给的是累积文本，所以直接覆盖这一条。 */
function setDraft(text, busy) {
  dropThinking();
  if (!draftNode) draftNode = appendMessage("assistant", "");
  draftNode.textContent = text;
  if (!busy) draftNode = null;
  scrollLog();
}

function clearLog() {
  el.log.replaceChildren();
  draftNode = null;
  thinkingNode = null;
  lastNoteText = "";
}

/** brain 的 effect → UI（和宠物窗口共用同一个 reducer，只是这里只关心文字） */
function applyEffects(effects) {
  for (const effect of effects) {
    if (effect.kind !== "say") continue;
    if (effect.messageKind === "status") pushNote(effect.text);
    else setDraft(effect.text, effect.busy);
  }
}

// ── 历史 ─────────────────────────────────────────────────────────────────
async function loadHistory() {
  if (!tauri) return;
  try {
    // 不传 sessionId = 跟随"当前对话"（Rust 侧读 current-session.json，没有就取最新）
    const messages = await tauri.core.invoke("load_history", { limit: 80, sessionId: null });
    clearLog();
    if (!Array.isArray(messages) || messages.length === 0) {
      appendMessage("note", "新对话，说点什么吧");
      return;
    }
    for (const message of messages) appendMessage(message.role, message.text);
    scrollLog();
  } catch {
    clearLog();
    appendMessage("note", "新对话，说点什么吧");
  }
}

// ── 提问 ─────────────────────────────────────────────────────────────────
async function ask(prompt) {
  if (!tauri) return;
  session = createSession();
  draftNode = null;
  lastNoteText = "";
  appendMessage("user", prompt);
  thinkingNode = appendMessage("note", "思考中…");
  try {
    await tauri.core.invoke("ask_deepcode", { prompt });
  } catch (error) {
    dropThinking();
    appendMessage("note", `连不上 deep-code：${String(error).replace(/^Error:\s*/, "")}`);
  }
}

async function wireBrain() {
  if (!tauri) return;
  await tauri.event.listen("brain", (event) => {
    const line = String(event.payload ?? "").trim();
    if (!line.startsWith("{")) return;
    let envelope;
    try {
      envelope = JSON.parse(line);
    } catch {
      return;
    }
    const item = envelope?.item ?? {};
    applyEffects(applyEnvelope(session, envelope));
    // 一轮结束的 print.result 里带 session_id：记下来，下次提问才续得上
    if (item.kind === "print.result") {
      const id = item.payload?.session_id;
      if (id) tauri.core.invoke("set_current_session", { id }).catch(() => {});
    }
  });
  await tauri.event.listen("brain-done", (event) => {
    dropThinking();
    applyEffects(finishTurn(session, { exitCode: Number(event.payload ?? 0) }));
  });
  // 右键菜单里切了对话（新建 / 打开历史）→ 重新读记录
  await tauri.event.listen("conversation-changed", () => {
    draftNode = null;
    thinkingNode = null;
    lastNoteText = "";
    loadHistory();
  });
}

// ── 交互 ─────────────────────────────────────────────────────────────────
function closePanel() {
  tauri?.core.invoke("close_chat").catch(() => {});
}

el.close.addEventListener("click", closePanel);

el.form.addEventListener("submit", (event) => {
  event.preventDefault();
  const text = el.input.value.trim();
  if (!text) return;
  el.input.value = "";
  autoGrow();
  el.send.disabled = true;
  ask(text).finally(() => {
    el.send.disabled = false;
    el.input.focus();
  });
});

el.input.addEventListener("input", autoGrow);
el.input.addEventListener("keydown", (event) => {
  // Enter 发送，Shift+Enter 换行（输入法组字期间不拦）
  if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
    event.preventDefault();
    el.form.requestSubmit();
  }
});

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") closePanel();
});

function autoGrow() {
  el.input.style.height = "auto";
  el.input.style.height = `${Math.min(el.input.scrollHeight, 96)}px`;
}

// 每次窗口被叫出来都回到最新一条，并聚焦输入框
window.addEventListener("focus", () => {
  scrollLog();
  el.input.focus();
});

// ── 启动 ─────────────────────────────────────────────────────────────────
wireBrain();
loadHistory();
el.input.focus();
