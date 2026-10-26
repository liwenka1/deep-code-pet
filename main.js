// 宠物窗口（main）：只负责"身体"——动画、气泡、拖动、换素材、把 brain 事件变成姿态。
// 对话文字在 chat.html 那个独立窗口里。两个窗口的好处：展开面板时宠物窗口
// 不需要 resize，所以宠物一个像素都不会动（之前"双击闪一下"的根因就是这个 resize）。

import { FRAME_H, FRAME_W, ONE_SHOT, atlasSize, frameRect } from "./pet-model.mjs";
import { createStateMachine } from "./pet-state.mjs";
import { parsePetJson, validateAtlas } from "./pet-loader.mjs";
import { applyEnvelope, createSession, finishTurn } from "./brain.mjs";

const el = {
  pet: document.getElementById("pet"),
  sprite: document.getElementById("sprite"),
  hint: document.getElementById("hint"),
  drop: document.getElementById("drop"),
};

const sm = createStateMachine();
const tauri = window.__TAURI__;
let atlas = { version: 1, width: 3 * FRAME_W, height: 3 * FRAME_H }; // 未加载前占位
let lastDraw = "";
let hasPet = false;
let hintTimer = 0;

// ── 尺寸 ─────────────────────────────────────────────────────────────────
const MIN_SCALE = 0.6;
const MAX_SCALE = 1.6;
const clampScale = (value) =>
  Math.min(MAX_SCALE, Math.max(MIN_SCALE, Math.round(value * 100) / 100));
// 键名带版本：改了默认值之后，老的 localStorage 值不会把新默认顶掉。
let scale = clampScale(Number(localStorage.getItem("pet-scale-v2")) || 0.7);
// 宠物贴住窗口的哪个角。改尺寸时靠它保持宠物不动。
const PAD = 8;
let anchor = "br";

function windowSizeFor(value) {
  // 窗口只比精灵大一圈（8px 边距），不再给气泡预留高度——
  // 预留的那片空白既看不见、会吃点击，还会让宠物永远差一截到不了屏幕顶。
  return {
    width: Math.round(FRAME_W * value + 16),
    height: Math.round(FRAME_H * value + 16),
  };
}

/** 改窗口尺寸时保持宠物的屏幕位置不变（由 Rust 侧挑方向）。 */
async function applyWindowLayout(width, height) {
  if (!tauri) return;
  try {
    anchor = await tauri.core.invoke("set_window_layout", {
      width,
      height,
      spriteW: FRAME_W * scale,
      spriteH: FRAME_H * scale,
      pad: PAD,
      anchor,
    });
    document.body.dataset.anchor = anchor;
  } catch {
    /* 拿不到窗口就维持现状 */
  }
}

function applyScale(next, { persist = true } = {}) {
  scale = clampScale(next);
  if (persist) localStorage.setItem("pet-scale-v2", String(scale));
  document.documentElement.style.setProperty("--sprite-w", `${Math.round(FRAME_W * scale)}px`);
  document.documentElement.style.setProperty("--sprite-h", `${Math.round(FRAME_H * scale)}px`);
  lastDraw = "";
  const size = windowSizeFor(scale);
  applyWindowLayout(size.width, size.height);
}

// ── 画精灵 ───────────────────────────────────────────────────────────────
function draw() {
  // 一次性/临时状态到点自动回落时，把"大脑想要的状态"也一并跟上，
  // 否则 baseState 会一直停在 jumping/review/waiting，下次点击又回那个状态。
  if (sm.tick()) baseState = sm.name;
  const index = sm.frameIndex();
  const rect = frameRect(sm.name, index, atlas.version);
  const key = `${sm.name}:${index}:${atlas.version}:${scale}`;
  if (key !== lastDraw) {
    lastDraw = key;
    el.sprite.style.backgroundSize = `${atlas.width * scale}px ${atlas.height * scale}px`;
    el.sprite.style.backgroundPosition = `${-rect.x * scale}px ${-rect.y * scale}px`;
  }
  requestAnimationFrame(draw);
}

// ── 通知：只用那张会自动消失的卡片（气泡已删除） ──────────────────────────
// 宠物是个 150×162 的透明窗，气泡只会挡脸、又读不了长文本，所以通知统一走
// 居中卡片：有宠物时它也照常出现，4 秒后自己消失。
function notify(text) {
  if (!text) return;
  el.hint.textContent = text.length > 120 ? `${text.slice(0, 120)}…` : text;
  el.hint.hidden = false;
  clearTimeout(hintTimer);
  hintTimer = setTimeout(() => {
    el.hint.hidden = true;
    showEmptyHint();
  }, 4000);
}

/**
 * 挥一次手：任何"打招呼"事件都可以调它，渲染方到点自己回 idle。
 */
function wave() {
  applyEffects([{ kind: "state", state: "waving" }]);
}

// ── 拖拽：位置由 OS 的全局光标坐标算 ────────────────────────────────────
// 早期实现用 JS 的 screenX 换算绝对坐标：跨显示器时坐标空间和 DPI 一变，
// 窗口就来回跳（表现为闪烁、拖不过去）。现在每次只让 Rust 做
// "窗口位置 = 全局光标 − 抓取偏移"，全程物理像素，跨屏交给 OS。
const DRAG_START_PX = 4;
let baseState = "idle"; // 大脑想要的状态
let dragState = null; // 拖拽期间的临时状态
let pointerDown = false;
let dragMoved = false;
let dragAccum = 0;
let dragFrames = 0;
let lastScreen = null;

// 一次性状态（jumping/waving/failed/review）不能当作"回到这里"的目标，
// 否则会自我循环（点几次之后一直跳，就是踩了这个坑）。
const restState = () => (ONE_SHOT.has(baseState) ? "idle" : baseState);

function beginDrag(direction) {
  dragState = direction;
  sm.set(direction);
}

function endDrag() {
  if (!dragState) return;
  dragState = null;
  sm.set(restState());
}

// 单击 / 双击的消歧：单击动作推迟 TAP_DELAY 才执行，第二击来了就取消它。
// 这样"双击"从头到尾不会先跳一下；代价是单击有 ~300ms 的延迟
// （想要零延迟就用悬停出现的小按钮，或右键菜单）。
const TAP_DELAY = 300;
let tapTimer = 0;

function tap() {
  if (tapTimer) {
    // 第二击落在这个窗口里 = 双击：取消单击动作，直接开对话
    clearTimeout(tapTimer);
    tapTimer = 0;
    wave();
    tauri?.core.invoke("open_chat").catch(() => {});
    return;
  }
  tapTimer = setTimeout(() => {
    tapTimer = 0;
    sm.set("jumping", { then: restState() });
  }, TAP_DELAY);
}

function cancelTap() {
  if (!tapTimer) return;
  clearTimeout(tapTimer);
  tapTimer = 0;
}

function pointerDragStart(event) {
  if (!tauri || event.button !== 0) return;
  cancelTap(); // 开始拖了，别再补一个"跳"
  pointerDown = true;
  dragMoved = false;
  dragAccum = 0;
  lastScreen = { x: event.screenX, y: event.screenY };
  tauri.core.invoke("drag_begin").catch(() => {});
}

function pointerDragMove(event) {
  if (!pointerDown) return;
  // 方向与阈值只用到"光标动了多少/往哪动"：优先 movementX（全局位移），
  // 某些 webview 在窗口跟随移动时会把它报成 0，这时退回 screenX 差值。
  const screenDx = lastScreen ? event.screenX - lastScreen.x : 0;
  const screenDy = lastScreen ? event.screenY - lastScreen.y : 0;
  lastScreen = { x: event.screenX, y: event.screenY };
  const dx = event.movementX || screenDx;
  const dy = event.movementY || screenDy;

  if (!dragMoved) {
    dragAccum += Math.abs(dx) + Math.abs(dy);
    if (dragAccum < DRAG_START_PX) return;
    dragMoved = true;
  }
  if (dx !== 0) beginDrag(dx > 0 ? "running-right" : "running-left");

  // 每帧最多一次 IPC；位置计算全在 Rust 侧（物理像素 + 当前显示器缩放）
  if (dragFrames) return;
  dragFrames = requestAnimationFrame(() => {
    dragFrames = 0;
    tauri.core.invoke("drag_update").catch(() => {});
  });
}

function pointerDragEnd() {
  if (!pointerDown) return;
  pointerDown = false;
  dragFrames = 0;
  tauri.core.invoke("drag_end").catch(() => {});
  if (dragMoved) {
    dragMoved = false;
    endDrag();
    // 每次拖完就落盘窗口位置（不再依赖 beforeunload，那条路径常常来不及）
    tauri.core.invoke("save_window_pos").catch(() => {});
    return;
  }
  // 没拖动 = 点击：可能是单击，也可能是双击的第一击 —— 交给 tap() 判
  tap();
}

function wirePointerDrag() {
  if (!tauri) return;
  el.pet.addEventListener("pointerdown", pointerDragStart);
  window.addEventListener("pointermove", pointerDragMove);
  window.addEventListener("pointerup", pointerDragEnd);
  window.addEventListener("pointercancel", pointerDragEnd);
  window.addEventListener("blur", pointerDragEnd);
}

// ── 素材 ─────────────────────────────────────────────────────────────────
function adoptPet({ json, imageUrl }) {
  const { pet, warnings } = parsePetJson(json);
  const hadPet = hasPet; // 换新宠物时挥个手，首次加载不额外挥（启动本来就挥）
  const image = new Image();
  image.onload = () => {
    let report;
    try {
      report = validateAtlas({ width: image.naturalWidth, height: image.naturalHeight }, pet.version);
    } catch (error) {
      reportProblem(`这只宠物不能用：${error.message}`);
      return;
    }
    atlas = { ...atlasSize(pet.version), version: pet.version };
    el.sprite.style.backgroundImage = `url(${imageUrl})`;
    el.sprite.classList.remove("empty");
    el.hint.hidden = true;
    hasPet = true;
    lastDraw = "";
    const notes = [...warnings, ...report.warnings];
    console.info(`[pet] ${pet.id} v${pet.version}`, notes);
    if (hadPet) wave(); // 换了一只宠物 = 新朋友打招呼
    console.info(`[pet] ${pet.id} v${pet.version}`, notes);
  };
  image.onerror = () => reportProblem("精灵图解码失败（webp/png 都可能）");
  image.src = imageUrl;
}

const EMPTY_HINT = "拖入宠物开始";

function showHint(text) {
  el.hint.textContent = text;
  el.hint.hidden = false;
}

/** 空状态（没宠物）用的默认文案：只有确实没有宠物时才铺满窗口。 */
function showEmptyHint() {
  if (hasPet) return;
  el.hint.innerHTML = EMPTY_HINT;
  el.hint.hidden = false;
  el.sprite.classList.add("empty");
}

/**
 * 出问题时怎么告诉用户：统一走那张会自动消失的卡片。
 * 没有宠物时卡片还会写回"拖入宠物开始"，有宠物时就单纯是一条 4 秒通知。
 */
function reportProblem(message) {
  notify(message);
}

/** Tauri 模式：文件夹路径交给 Rust 读。返回是否成功（失败时可回退到别的宠物）。 */
async function loadFromPath(dir) {
  if (!tauri) return false;
  try {
    const payload = await tauri.core.invoke("load_pet", { dir });
    const bytes = Uint8Array.from(atob(payload.image_base64), (c) => c.charCodeAt(0));
    const url = URL.createObjectURL(new Blob([bytes], { type: payload.mime }));
    adoptPet({ json: payload.json, imageUrl: url });
    // 记住这一只：下次启动还是它
    localStorage.setItem("pet-dir", dir);
    clearTimeout(hintTimer);
    el.hint.hidden = true;
    hasPet = true;
    return true;
  } catch (error) {
    reportProblem(String(error).replace(/^Error:\s*/, ""));
    return false;
  }
}

/** 浏览器模式：只支持 HTML5 拖入（Tauri 里走 onDragDropEvent） */
function wireBrowserDrop() {
  document.addEventListener("dragover", (event) => event.preventDefault());
  document.addEventListener("drop", async (event) => {
    event.preventDefault();
    const item = [...(event.dataTransfer?.items ?? [])].find((i) => i.kind === "file");
    const entry = item?.webkitGetAsEntry?.();
    if (!entry) return showHint("拖一个文件夹进来");
    if (!entry.isDirectory) return showHint("请拖整只宠物文件夹");
    const files = await readAllFiles(entry);
    const jsonFile = files.find((f) => f.name === "pet.json");
    if (!jsonFile) return showHint("这个文件夹里没有 pet.json");
    const json = await jsonFile.text();
    let sheetName;
    try {
      sheetName = JSON.parse(json).spritesheetPath;
    } catch {
      return showHint("pet.json 解析失败");
    }
    const sheet = files.find((f) => f.name === sheetName);
    if (!sheet) return showHint(`文件夹里没有 ${sheetName}`);
    adoptPet({ json, imageUrl: URL.createObjectURL(sheet) });
  });
}

function readAllFiles(entry, out = []) {
  return new Promise((resolve) => {
    if (entry.isFile) {
      entry.file((file) => {
        out.push(file);
        resolve(out);
      });
    } else if (entry.isDirectory) {
      const reader = entry.createReader();
      const readBatch = () => {
        reader.readEntries(async (batch) => {
          if (batch.length === 0) return resolve(out);
          for (const child of batch) await readAllFiles(child, out);
          readBatch();
        });
      };
      readBatch();
    } else resolve(out);
  });
}

// ── 大脑：只把事件变成姿态（文字在 chat 窗口里显示） ──────────────────────
let session = createSession();

function applyEffects(effects) {
  for (const effect of effects) {
    if (effect.kind === "state") {
      baseState = effect.state;
      // transient = 本轮的一次性提示（比如"本来要问你"），到点自动回落
      if (!dragState) {
        sm.set(effect.state, effect.transient === true ? { autoReturn: true } : {});
      }
    }
    // 文字不进宠物窗口：正文与状态说明都在 chat 窗口里显示。
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
    // 一次回合开始时重置累积器（回合由 chat 窗口发起，宠物这边只跟随）
    if (envelope?.item?.kind === "turn.started") session = createSession();
    applyEffects(applyEnvelope(session, envelope));
  });
  await tauri.event.listen("brain-done", (event) => {
    applyEffects(finishTurn(session, { exitCode: Number(event.payload ?? 0) }));
  });
  // 新建 / 切换对话：打个招呼再开始
  await tauri.event.listen("conversation-changed", () => {
    session = createSession();
    wave();
  });
}

// ── 交互 ─────────────────────────────────────────────────────────────────
// 打开对话 = 悬停出现的小按钮，或右键菜单。
// 不再用双击：单击/双击的判定窗口（系统设置，默认 0.5s）和"点一下就要有反应"
// 天生冲突——延迟就是为了等第二击，不等就必然误触发。
const chatBtn = document.getElementById("chat-btn");
chatBtn.addEventListener("pointerdown", (event) => event.stopPropagation());
chatBtn.addEventListener("pointerup", (event) => event.stopPropagation());
chatBtn.addEventListener("click", (event) => {
  event.stopPropagation();
  cancelTap(); // 刚点过宠物本体也没关系，别再补一个蹦
  wave(); // 被叫到了，先招个手
  tauri?.core.invoke("open_chat").catch(() => {});
});

// 右键 → 原生菜单（历史对话 / 新建对话 / 打开对话 / 退出）
el.pet.addEventListener("contextmenu", (event) => {
  event.preventDefault();
  tauri?.core.invoke("show_pet_menu").catch(() => {});
});

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    el.hint.hidden = true;
    return;
  }
  if (!(event.metaKey || event.ctrlKey)) return;
  if (event.shiftKey && (event.key === "r" || event.key === "R")) {
    // 新开一条对话（不删历史）：立即清掉本窗口的累积状态并叫出面板
    event.preventDefault();
    session = createSession();
    tauri?.core.invoke("new_conversation").catch(() => {});
    return;
  }
  if (event.key === "-" || event.key === "_") {
    event.preventDefault();
    applyScale(scale - 0.1);
  } else if (event.key === "=" || event.key === "+") {
    event.preventDefault();
    applyScale(scale + 0.1);
  }
});

// ── 启动 ─────────────────────────────────────────────────────────────────
applyScale(scale, { persist: false });
draw();
el.sprite.classList.add("empty");

if (tauri) {
  wireBrain();
  wirePointerDrag();

  // 启动顺序：上次拖入的那只 → 素材目录里的第一只 → 让用户拖一只进来
  const remembered = localStorage.getItem("pet-dir");
  const boot = async () => {
    if (remembered && (await loadFromPath(remembered))) return;
    const pets = await tauri.core.invoke("list_pets").catch(() => []);
    if (pets.length > 0 && (await loadFromPath(pets[0].dir))) return;
    showEmptyHint();
  };
  boot();

  // 拖入换宠物：enter/over 给个提示层，drop 时换，leave 收起
  tauri.webview.getCurrentWebview().onDragDropEvent((event) => {
    const type = event.payload?.type;
    if (type === "enter" || type === "over") {
      el.drop.hidden = false;
      return;
    }
    if (type === "leave") {
      el.drop.hidden = true;
      return;
    }
    if (type !== "drop") return;
    el.drop.hidden = true;
    const dir = event.payload.paths?.[0];
    if (!dir) return;
    loadFromPath(dir).then((ok) => {
      if (!ok) reportProblem("这只好像不对，再换一只？");
    });
  });

  // 体检：key 从哪来、模型是什么。缺 key 就直接说清楚，而不是等用户看到"出错了"。
  tauri.core
    .invoke("doctor_summary")
    .then((report) => {
      const source = report?.api_key?.source ?? "missing";
      console.info(
        `[pet] deepcode ${report?.version} · key=${source} · model=${report?.default_model} · config=${report?.config_path}`,
      );
      if (source === "missing") {
        notify("我还没连上大脑：先在终端跑一次 deepcode 把 key 配好");
      }
    })
    .catch(() => {
      console.error("[pet] 找不到 deepcode，PATH 里没有");
      if (hasPet) {
        notify("我找不到 deepcode，先在终端确认 `deepcode --version` 能跑");
      }
    });

  // 让宠物打个招呼
  setTimeout(wave, 400);
  window.addEventListener("beforeunload", () => {
    tauri.core.invoke("save_window_pos").catch(() => {});
  });
  tauri.core.invoke("restore_window_pos").catch(() => {});
} else {
  wireBrowserDrop();
  showHint("拖入宠物开始");
}
