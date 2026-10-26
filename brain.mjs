// deep-code 的 NDJSON envelope → 宠物该做什么。
// envelope 契约：crates/deep-code-runtime/src/events.rs:21-38（`.item.kind` / `.item.payload.*`）
// kind 全表：crates/deep-code-agent/src/runtime/event.rs:218-242
//
// 纯 reducer：把 stdout 的每一行喂进 applyEnvelope，拿回 effect 列表，
// UI 层照着执行（换状态 / 显示气泡）。不碰进程、不碰 DOM。

export const BUBBLE_THROTTLE_MS = 350;
export const BUBBLE_MAX_CHARS = 400;

export function createSession() {
  return {
    assistant: "",
    reasoning: "",
    toolCalls: 0,
    lastBubbleAt: 0,
    terminal: null, // "completed" | "cancelled" | "failed"
    result: null, // print.result 的最终报告
    spokeError: false, // 本轮是否已经用气泡报过错了（避免收尾再说一遍）
  };
}

const clamp = (text) =>
  text.length > BUBBLE_MAX_CHARS ? `${text.slice(0, BUBBLE_MAX_CHARS)}…` : text;

/**
 * @returns {Array<{kind:"state",state:string}|{kind:"say",text:string,busy:boolean,messageKind:string,status?:string}>}
 */
export function applyEnvelope(session, envelope, now = Date.now()) {
  const item = envelope?.item ?? envelope ?? {};
  const kind = item.kind ?? "";
  const payload = item.payload ?? {};
  const effects = [];

  switch (kind) {
    case "turn.started":
      effects.push({ kind: "state", state: "running" });
      break;

    case "reasoning.delta":
      session.reasoning += payload.text ?? "";
      effects.push({ kind: "state", state: "running" });
      break;

    case "assistant.delta": {
      session.assistant += payload.text ?? "";
      effects.push({ kind: "state", state: "running" });
      if (now - session.lastBubbleAt > BUBBLE_THROTTLE_MS) {
        session.lastBubbleAt = now;
        effects.push({
          kind: "say",
          text: clamp(session.assistant),
          busy: true,
          messageKind: "assistant",
          status: "running",
        });
      }
      break;
    }

    case "tool.started":
      session.toolCalls += 1;
      effects.push({ kind: "state", state: "running" });
      if (payload.tool_name) {
        effects.push({
          kind: "say",
          text: `用一下 ${payload.tool_name}`,
          busy: true,
          messageKind: "status",
          status: "running",
        });
      }
      break;

    case "approval.required":
      effects.push({ kind: "state", state: "waiting" });
      effects.push({
        kind: "say",
        text: "这个要你点头才能做",
        busy: false,
        messageKind: "status",
        status: "needs_input",
      });
      break;

    case "approval.resolved":
      // 批准或拒绝之后都要离开 waiting，否则宠物会一直停在「等你点头」。
      effects.push({ kind: "state", state: "running" });
      break;

    case "compaction.started":
      effects.push({
        kind: "say",
        text: "我把前面的事捋一遍…",
        busy: true,
        messageKind: "status",
        status: "running",
      });
      break;

    case "warning":
      effects.push({
        kind: "say",
        text: payload.message ?? "有点小状况",
        busy: false,
        messageKind: "status",
      });
      break;

    case "error":
      session.terminal = "failed";
      session.spokeError = true;
      effects.push({ kind: "state", state: "failed" });
      effects.push({
        kind: "say",
        text: payload.message ?? "出错了",
        busy: false,
        messageKind: "status",
      });
      break;

    case "turn.completed":
      session.terminal = "completed";
      break;

    case "turn.cancelled":
      session.terminal = "cancelled";
      break;

    case "print.result":
      session.result = payload;
      break;

    default:
      // session.updated / checkpoint.created / tool.finished … v0.1 不表现
      break;
  }

  return effects;
}

/**
 * 一轮收尾。有工具调用 → review（展示做过什么）；纯对话 → jumping（庆祝）。
 */
export function finishTurn(session, { exitCode = 0 } = {}) {
  const report = session.result;
  const failed = session.terminal === "failed" || (exitCode !== 0 && !report);
  const status = report?.status ?? (failed ? "error" : (session.terminal ?? "finished"));
  const text = (report?.result || session.assistant || "").trim();
  const effects = [];

  if (status === "error" || status === "timeout") {
    if (!session.terminal) effects.push({ kind: "state", state: "failed" });
    // 同一个错误只说一次：applyEnvelope 的 error 分支已经说过了就不再重复。
    if (!session.spokeError) {
      effects.push({
        kind: "say",
        text: report?.error || text || (status === "timeout" ? "这一轮超时了" : "出错了"),
        busy: false,
        messageKind: "status",
      });
    }
    return effects;
  }

  if (status === "cancelled") {
    effects.push({ kind: "state", state: "idle" });
    effects.push({ kind: "say", text: "好，停下了", busy: false, messageKind: "status" });
    return effects;
  }

  // 本轮有调用「本来该问人、但没人能回答」而被自动拒绝 —— `-p` 是一次性进程，
  // 永远是这种无人值守姿态，所以这也是 waiting 这个状态**唯一**能被触发的真实场景。
  const denied = Number(report?.denied_approvals ?? 0);
  if (denied > 0) {
    effects.push({ kind: "state", state: "waiting", transient: true });
    effects.push({
      kind: "say",
      text: `刚才有 ${denied} 步需要你点头，可没人能回答`,
      busy: false,
      messageKind: "status",
    });
    if (text) {
      effects.push({
        kind: "say",
        text: clamp(text),
        busy: false,
        messageKind: "assistant",
        status: "idle",
      });
    }
    return effects;
  }

  effects.push({ kind: "state", state: session.toolCalls > 0 ? "review" : "jumping" });
  if (text) {
    effects.push({
      kind: "say",
      text: clamp(text),
      busy: false,
      messageKind: "assistant",
      status: "idle",
    });
  }
  return effects;
}
