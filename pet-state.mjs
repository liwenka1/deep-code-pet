// 宠物状态机：谁决定现在该演哪一帧。纯逻辑，不碰 DOM。

import { ONE_SHOT, STATES, byName, cycleMs, frameIndexAt } from "./pet-model.mjs";

export const IDLE = "idle";

/**
 * 一次性状态停留多久之后自动回 idle。
 * 默认 = 该状态一轮动画的时长；个别状态给更长的停留。
 */
function dwellMs(name) {
  if (name === "waving") return 1500; // 打招呼挥两下就收
  if (name === "review") return 2000; // 展示「我刚做了什么」，停两秒再回 idle
  if (name === "waiting") return 3000; // 只在本轮收尾「本来要问你」时当临时提示用
  return cycleMs(name);
}

/** @param {() => number} now 注入时钟，测试好控制 */
export function createStateMachine(now = () => Date.now()) {
  let current = IDLE;
  let since = now();
  let returnTo = IDLE;
  let autoReturn = false;
  let dwell = 0;

  const api = {
    get name() {
      return current;
    },
    elapsed() {
      return now() - since;
    },
    frameIndex() {
      return frameIndexAt(current, api.elapsed());
    },
    /**
     * 切状态。同状态重复设置被忽略（不打断循环动画），除非 force。
     *
     * `autoReturn` 默认沿用「一次性状态播完回 idle」的规则；个别调用可以
     * 显式让一个**循环**状态也临时自动回落（例如 waiting 当作本轮提示时）。
     *
     * @returns {boolean} 是否真的切了
     */
    set(name, { force = false, then = IDLE, autoReturn: auto = ONE_SHOT.has(name) } = {}) {
      if (!byName(name)) throw new Error(`unknown state: ${name}`);
      if (!force && name === current) return false;
      current = name;
      since = now();
      returnTo = then;
      autoReturn = auto;
      dwell = dwellMs(name);
      return true;
    },
    /** 推进时钟：到点回 returnTo。变了返回 true。 */
    tick() {
      if (!autoReturn) return false;
      if (api.elapsed() < dwell) return false;
      current = returnTo;
      since = now();
      autoReturn = false;
      return true;
    },
    states: STATES,
  };

  return api;
}
