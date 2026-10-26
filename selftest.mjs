// 离线自测：node selftest.mjs
// 覆盖四块纯逻辑（素材模型 / 状态机 / 素材校验 / 事件翻译），不需要 Tauri、
// 不需要网络、不需要 deep-code。

import { COLS, FRAME_H, FRAME_W, STATES, atlasSize, cycleMs, frameIndexAt, frameRect } from "./pet-model.mjs";
import { createStateMachine } from "./pet-state.mjs";
import { parsePetJson, validateAtlas } from "./pet-loader.mjs";
import { applyEnvelope, createSession, finishTurn } from "./brain.mjs";

let failures = 0;
const ok = (cond, msg, extra) => {
  if (cond) {
    console.log(`ok   ${msg}`);
  } else {
    failures += 1;
    console.error(`FAIL ${msg}${extra ? ` — ${JSON.stringify(extra)}` : ""}`);
  }
};
const throws = (fn, msg) => {
  try {
    fn();
    failures += 1;
    console.error(`FAIL ${msg}（没有抛错）`);
  } catch {
    console.log(`ok   ${msg}`);
  }
};

console.log("── 素材模型 ─────────────────────────────────────");
ok(COLS * FRAME_W === 1536, "atlas 宽 1536");
ok(atlasSize(1).height === 1872, "v1 高 1872 (8×9)");
ok(atlasSize(2).height === 2288, "v2 高 2288 (8×11)");
ok(STATES.length === 9, "九个状态");
ok(STATES.every((s, i) => s.row === i), "行号 0..8 与表顺序一致");
ok(STATES.every((s) => s.frames.length <= COLS), "每个状态帧数 ≤ 8 列");
ok(cycleMs("idle") === 1100, "idle 一轮 1100ms");
ok(cycleMs("failed") === 1220, "failed 一轮 1220ms");

const r = frameRect("waiting", 2, 2);
ok(r.x === 384 && r.y === 1248 && r.width === 192 && r.height === 208, "waiting 第2帧矩形");
ok(frameIndexAt("idle", 0) === 0, "idle t=0 → 帧0");
ok(frameIndexAt("idle", 279) === 0, "idle t=279 → 帧0");
ok(frameIndexAt("idle", 280) === 1, "idle t=280 → 帧1（首帧 280ms）");
ok(frameIndexAt("idle", 1100) === 0, "idle 一轮后回帧0");
ok(frameIndexAt("failed", cycleMs("failed") + 1) === 0, "取模不越界");
throws(() => frameRect("nope", 0), "未知状态抛错");

console.log("\n── 状态机 ───────────────────────────────────────");
{
  let clock = 0;
  const sm = createStateMachine(() => clock);
  ok(sm.name === "idle", "初始 idle");
  ok(sm.set("running") === true, "idle → running 生效");
  ok(sm.set("running") === false, "重复设置同状态被忽略");
  ok(sm.set("jumping") === true, "跳到 jumping");
  clock += 839;
  ok(sm.tick() === false, "jumping 未播完不切");
  clock += 1;
  ok(sm.tick() === true && sm.name === "idle", "jumping 播完自动回 idle");
  ok(sm.set("failed") === true, "切到 failed");
  clock += 1219;
  ok(sm.tick() === false, "failed 不到 1220ms 保持");
  clock += 1;
  ok(sm.tick() === true && sm.name === "idle", "failed 播完回 idle");
  ok(sm.set("waiting") === true, "waiting 是循环状态");
  clock += 100000;
  ok(sm.tick() === false && sm.name === "waiting", "循环状态不会被 tick 打断");

  // waiting 显式当"临时提示"用时也会回落（本轮"本来要问你"那种）
  ok(sm.set("idle") === true, "先回到 idle");
  ok(sm.set("waiting", { autoReturn: true, then: "idle" }) === true, "waiting 可显式临时化");
  clock += 2999;
  ok(sm.tick() === false, "临时 waiting 3s 内保持");
  clock += 1;
  ok(sm.tick() === true && sm.name === "idle", "临时 waiting 3s 后自动回落");

  ok(sm.set("review") === true, "切到 review（展示成果）");
  clock += 1029;
  ok(sm.tick() === false && sm.name === "review", "review 不到 2000ms 不回落");
  clock += 971;
  ok(sm.tick() === true && sm.name === "idle", "review 停 2s 后自动回 idle");

  ok(sm.set("waving") === true, "切到 waving（打招呼）");
  clock += 1499;
  ok(sm.tick() === false && sm.name === "waving", "waving 1.5s 内保持");
  clock += 1;
  ok(sm.tick() === true && sm.name === "idle", "waving 1.5s 后自己收手（不会一直挥）");
  throws(() => sm.set("dancing"), "未知状态抛错");
}

console.log("\n── 素材校验 ─────────────────────────────────────");
{
  const sample = JSON.stringify({
    id: "demo",
    displayName: "Demo",
    description: "测试用素材。",
    spriteVersionNumber: 2,
    spritesheetPath: "spritesheet.webp",
  });
  const { pet, warnings } = parsePetJson(sample);
  ok(pet.version === 2 && pet.id === "demo", "解析示例 pet.json");
  ok(warnings.length === 0, "示例素材无警告");

  ok(parsePetJson(JSON.stringify({ ...JSON.parse(sample), spriteVersionNumber: undefined })).pet.version === 1, "缺版本号按 v1");
  ok(parsePetJson(JSON.stringify({ ...JSON.parse(sample), spriteVersionNumber: 7 })).warnings.length === 1, "未知版本号给警告");
  throws(() => parsePetJson("not json"), "非法 JSON 抛错");
  throws(() => parsePetJson(JSON.stringify({ id: "x", displayName: "X" })), "缺 spritesheetPath 抛错");

  ok(validateAtlas({ width: 1536, height: 1872 }, 1).warnings.length === 0, "1536×1872 声明 v1 干净");
  ok(validateAtlas({ width: 1536, height: 2288 }, 2).warnings.length === 0, "1536×2288 声明 v2 干净");
  ok(validateAtlas({ width: 1536, height: 2288 }, 1).warnings.length === 1, "2288 声明 v1 → 提醒多出的行");
  ok(validateAtlas({ width: 1536, height: 1872 }, 2).warnings.length === 1, "1872 声明 v2 → 提醒缺行");
  ok(validateAtlas({ width: 1600, height: 1872 }, 1).warnings.length >= 1, "宽度不是 192 倍数 → 提醒错位");
  throws(() => validateAtlas({ width: 128, height: 128 }, 1), "低于 256 → 抛错");
}

console.log("\n── 事件翻译（brain）─────────────────────────────");
{
  // 一整套真实形状的 envelope（字段与 runtime/event.rs 一致）
  const t = "turn_demo_0";
  const frames = [
    ["user.message", { content: "你好呀" }],
    ["turn.started", { turn_id: t, prompt: "你好呀" }],
    ["reasoning.delta", { turn_id: t, text: "想想要怎么打招呼" }],
    ["assistant.delta", { turn_id: t, text: "唔…" }],
    ["tool.started", { turn_id: t, tool_call_id: "c1", tool_name: "read_file", arguments: {} }],
    ["tool.finished", { turn_id: t, tool_call_id: "c1", result: { status: "success" } }],
    ["assistant.delta", { turn_id: t, text: "我是噜噜。" }],
    ["turn.completed", { turn_id: t, usage: null }],
    ["print.result", { status: "finished", result: "唔…我是噜噜。", denied_approvals: 0, duration_ms: 812 }],
  ].map(([kind, payload], i) => ({ seq: i + 1, item: { kind, payload } }));

  const session = createSession();
  const effects = [];
  let now = 0;
  for (const envelope of frames) {
    effects.push(...applyEnvelope(session, envelope, now));
    now += 400; // 越过节流窗口，让增量气泡每次都发
  }
  ok(effects.some((e) => e.kind === "state" && e.state === "running"), "turn.started → running");
  ok(effects.filter((e) => e.kind === "say" && e.messageKind === "assistant").length >= 1, "assistant.delta → 气泡");
  ok(effects.some((e) => e.kind === "say" && e.text.includes("read_file")), "tool.started → 工具气泡");
  ok(session.toolCalls === 1, "工具计数 1");
  ok(session.result?.status === "finished", "print.result 落到 session");

  const tail = finishTurn(session, { exitCode: 0 });
  ok(tail.some((e) => e.kind === "state" && e.state === "review"), "有工具调用 → review 收尾");
  ok(tail.some((e) => e.kind === "say" && e.text === "唔…我是噜噜。"), "收尾说出完整一句");

  const chatOnly = createSession();
  applyEnvelope(chatOnly, { item: { kind: "assistant.delta", payload: { text: "嗯？" } } }, 0);
  const tail2 = finishTurn(chatOnly, { exitCode: 0 });
  ok(tail2.some((e) => e.kind === "state" && e.state === "jumping"), "纯对话 → jumping 收尾");

  // 正文不截断：长度限制只属于各自的 UI（宠物提示卡片 120 字，聊天面板全文）。
  // 以前 reducer 里按 400 字砍，导致聊天面板里的长回复也被截成半句。
  const longText = "喵".repeat(600);
  const streamed = createSession();
  const streamEffects = applyEnvelope(
    streamed,
    { item: { kind: "assistant.delta", payload: { text: longText } } },
    1000, // 越过节流窗口，否则压根不会发气泡
  );
  ok(
    streamEffects.some((e) => e.kind === "say" && e.text.length === 600),
    "流式增量整段带回（不再被截到 400 字）",
  );
  const tailLong = finishTurn(streamed, { exitCode: 0 });
  ok(
    tailLong.some((e) => e.kind === "say" && e.messageKind === "assistant" && e.text.length === 600),
    "收尾的完整回复也不截断",
  );

  const failed = createSession();
  const failEffects = applyEnvelope(failed, { item: { kind: "error", payload: { message: "API 挂了" } } }, 0);
  const tail3 = finishTurn(failed, { exitCode: 1 });
  const spoken = [...failEffects, ...tail3].filter((e) => e.kind === "say");
  ok(
    spoken.length === 1 && spoken[0].text === "API 挂了",
    "error → 只报一次原因（收尾不重复说）",
    spoken,
  );
  ok(tail3.every((e) => e.kind !== "state" || e.state !== "failed"), "error 的 failed 状态不重复推");

  const cancelled = createSession();
  applyEnvelope(cancelled, { item: { kind: "turn.cancelled", payload: {} } }, 0);
  const tail4 = finishTurn(cancelled, { exitCode: 0 });
  ok(tail4.some((e) => e.kind === "say" && e.text === "好，停下了"), "cancel → 一句收场");

  // 进程非零退出、一个事件都没有 → 也算失败，不能摆出「做完了」的姿势
  const silent = createSession();
  const tail5 = finishTurn(silent, { exitCode: 1 });
  ok(tail5.some((e) => e.kind === "state" && e.state === "failed"), "静默失败 → failed");

  // timeout 也算失败（以前会落到"完成"分支，是个漏洞）
  const timedOut = createSession();
  timedOut.result = { status: "timeout", result: "", denied_approvals: 0 };
  const tail6 = finishTurn(timedOut, { exitCode: 0 });
  ok(tail6.some((e) => e.kind === "state" && e.state === "failed"), "status=timeout → failed");

  // 有调用被自动拒绝（没人能回答）→ waiting（transient），这是 waiting 唯一的真实触发点
  const deniedTurn = createSession();
  deniedTurn.result = { status: "finished", result: "我试过了", denied_approvals: 2 };
  const tail7 = finishTurn(deniedTurn, { exitCode: 0 });
  ok(
    tail7.some((e) => e.kind === "state" && e.state === "waiting" && e.transient === true),
    "denied_approvals>0 → waiting(transient)",
  );
  ok(
    tail7.some((e) => e.kind === "say" && e.text.includes("2 步")),
    "denied_approvals 会说明有几步",
  );

  // approval.required → waiting
  const waiting = createSession();
  const w = applyEnvelope(waiting, { item: { kind: "approval.required", payload: { request: {} } } }, 0);
  ok(w.some((e) => e.kind === "state" && e.state === "waiting"), "approval.required → waiting");
  const resolved = applyEnvelope(waiting, { item: { kind: "approval.resolved", payload: { decision: "approved" } } }, 0);
  ok(resolved.some((e) => e.kind === "state" && e.state === "running"), "approval.resolved → 离开 waiting");
}

console.log(failures === 0 ? "\n全部通过" : `\n${failures} 条失败`);
process.exit(failures === 0 ? 0 : 1);
