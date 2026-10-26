// 精灵图集（atlas）模型：九行状态、每行帧数、逐帧停留时长。
// 「兼容一种素材格式」= 实现这张表，而不是为每只宠物写适配。

export const FRAME_W = 192;
export const FRAME_H = 208;
export const COLS = 8; // 固定 8 列，帧在行内左对齐

/** row: atlas 行号；frames: 每帧停留毫秒（数组长度 = 该状态帧数） */
export const STATES = [
  { name: "idle", row: 0, frames: [280, 110, 110, 140, 140, 320] },
  { name: "running-right", row: 1, frames: [120, 120, 120, 120, 120, 120, 120, 220] },
  { name: "running-left", row: 2, frames: [120, 120, 120, 120, 120, 120, 120, 220] },
  { name: "waving", row: 3, frames: [140, 140, 140, 280] },
  { name: "jumping", row: 4, frames: [140, 140, 140, 140, 280] },
  { name: "failed", row: 5, frames: [140, 140, 140, 140, 140, 140, 140, 240] },
  { name: "waiting", row: 6, frames: [150, 150, 150, 150, 150, 260] },
  { name: "running", row: 7, frames: [120, 120, 120, 120, 120, 220] },
  { name: "review", row: 8, frames: [150, 150, 150, 150, 150, 280] },
];

/**
 * 播完要自动回到 idle 的状态；其余循环播放。
 * 判据不是"像不像一次性动作"，而是"没有人会主动把它关掉"——
 * waving / jumping / failed / review 都属于这类，漏一个就会永远卡在那里。
 */
export const ONE_SHOT = new Set(["waving", "jumping", "failed", "review"]);

export const byName = (name) => STATES.find((s) => s.name === name);

/** v1 = 8×9（1536×1872）；v2 = 8×11（1536×2288，多两行留给消费方）。 */
export function atlasSize(version = 1) {
  const rows = version >= 2 ? 11 : 9;
  return { width: COLS * FRAME_W, height: rows * FRAME_H, rows };
}

/** 某一帧在 atlas 里的矩形。 */
export function frameRect(state, index, version = 1) {
  const def = typeof state === "string" ? byName(state) : state;
  if (!def) throw new Error(`unknown state: ${state}`);
  const { rows } = atlasSize(version);
  if (def.row >= rows) {
    throw new Error(`state ${def.name} 行 ${def.row} 超出 ${rows} 行的 atlas`);
  }
  const i = ((index % def.frames.length) + def.frames.length) % def.frames.length;
  return { x: i * FRAME_W, y: def.row * FRAME_H, width: FRAME_W, height: FRAME_H };
}

export const cycleMs = (state) => byName(state).frames.reduce((a, b) => a + b, 0);

/** 播放到 elapsedMs 时该显示第几帧（不规则时长也能正确取模）。 */
export function frameIndexAt(state, elapsedMs) {
  const def = typeof state === "string" ? byName(state) : state;
  const total = def.frames.reduce((a, b) => a + b, 0);
  let t = ((elapsedMs % total) + total) % total;
  for (let i = 0; i < def.frames.length; i += 1) {
    if (t < def.frames[i]) return i;
    t -= def.frames[i];
  }
  return def.frames.length - 1;
}
