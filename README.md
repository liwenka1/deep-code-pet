# deep-code-pet

**桌面宠物。** 读本地宠物素材，用 deep-code 当大脑。

一只浮在桌面上的宠物：点它会说话，回答来自你本机装的 `deepcode`；
它干活、出错、需要你点头时，有对应姿态。

---

## 现在能做什么

| 能力 | 状态 |
|---|---|
| 读宠物素材（`pet.json` + `spritesheet.webp`，v1/v2 图集） | ✅ |
| 九状态动画，逐帧时长按规格表（含 idle 不规则眨眼） | ✅ |
| 拖一个宠物文件夹进去就用；也自动扫 `~/.deep-code-pet/pets` | ✅ |
| 拖入即换宠物，并**记住这一只**（下次启动还是它） | ✅ |
| 一只都没有时提示「拖入宠物开始」 | ✅ |
| 单击/双击宠物 → 打开**独立的聊天窗口**（完整记录 + 多行输入 + 流式渲染） | ✅ |
| 工具调用 / 出错 / 一轮结束 → 换姿态；文字进聊天窗口 | ✅ |
| 历史对话窗口：列表切换、新建、当前标记 | ✅ |
| 窗口位置记忆、透明无边框置顶、可拖到菜单栏之上 | ✅ |
| 拖窗口时朝拖动方向跑（`running-right` / `running-left`），松手回原状态 | ✅ |
| 尺寸可调：默认 0.7×（窗口 150×162），`Cmd/Ctrl` + `-` / `=` 缩放并记忆 | ✅ |
| 点击穿透（透明区域把点击还给下层 app） | ❌ 见"已知限制" |
| 审批按钮闭环（在面板里点同意/拒绝） | ❌ 下一版 |

所有纯逻辑（素材模型 / 状态机 / 素材校验 / 事件翻译）都有离线自测：

```bash
node selftest.mjs      # 或 pnpm test
```

---

## 快速开始

```bash
# 0. 前提：本机已装 deepcode（`deepcode --version` 有输出），Node 20+
pnpm install

# 1. 先验纯逻辑（不需要 Tauri、不需要网络）
pnpm test

# 2. 放一只宠物素材，或起来之后直接把文件夹拖进窗口
mkdir -p ~/.deep-code-pet/pets && cp -R /path/to/some-pet ~/.deep-code-pet/pets/

# 3. 起来看看
pnpm app        # = tauri dev
```

第一次 `tauri dev` 会编译 Rust（几分钟）。起来后应该看到宠物站在窗口底部、打个招呼（`waving`），
鼠标移上去会浮出一个小气泡按钮，点它（或双击宠物）打开聊天窗口。

只想先看动画、不想装 Rust 工具链？`pnpm dev` 然后在浏览器打开
<http://localhost:1420>，把宠物文件夹拖进去——浏览器模式只能演动画，不能连 deep-code。

---

## 素材格式

一个文件夹，两个文件：`pet.json` + `spritesheet.webp`（或 `.png`）。

| 项 | 值 |
|---|---|
| 帧尺寸 / 列数 | 192×208，**固定 8 列**，帧在行内左对齐 |
| v1 图集 | 1536×1872（9 行） |
| v2 图集 | 1536×2288（11 行；多出的 2 行留给消费方） |
| 下限 | 图 ≥ 256×256 |

九行状态（行号即数组下标，不可重排；箭头后是逐帧停留毫秒）：

| 状态 | 行 | 帧数 | 逐帧时长 (ms) |
|---|---|---|---|
| `idle` | 0 | 6 | 280, 110, 110, 140, 140, 320（不规则眨眼） |
| `running-right` | 1 | 8 | 120 ×7, 220 |
| `running-left` | 2 | 8 | 120 ×7, 220 |
| `waving` | 3 | 4 | 140 ×3, 280 |
| `jumping` | 4 | 5 | 140 ×4, 280 |
| `failed` | 5 | 8 | 140 ×7, 240 |
| `waiting` | 6 | 6 | 150 ×5, 260 |
| `running` | 7 | 6 | 120 ×5, 220 |
| `review` | 8 | 6 | 150 ×5, 280 |

`pet.json` 五个字段，缺 `id` / `displayName` / `spritesheetPath` 会拒绝加载：

```json
{
  "id": "demo",
  "displayName": "Demo",
  "description": "一句话描述，可省略。",
  "spriteVersionNumber": 2,
  "spritesheetPath": "spritesheet.webp"
}
```

`spriteVersionNumber` 决定 9 行还是 11 行，缺失按 v1；其它字段一律忽略。
播放约定：`waving` / `jumping` / `failed` / `review` 播完自动回 `idle`，其余循环。

三种放置方式，任选：

1. **拖进窗口**（推荐，任意路径，路径会被记住）；
2. 放到 `~/.deep-code-pet/pets/<名字>/`，启动时自动扫；
3. `DEEP_CODE_PET_ASSET=/path/to/pet pnpm app` 直接指定一只。

拖错东西（比如拖了个 zip 或单个 png）会直接告诉你"这不是文件夹 / 里面没有 pet.json"，
不会静默失败。

---

## 人格

人格 = `AGENTS.md`。首次启动会写一份默认的到：

```
~/Library/Application Support/dev.deepcode.pet/agent/AGENTS.md
```

改这个文件就改性格（默认写的是一只叫"噜噜"的水豚：说短句、不碰工具）。
会话也落在这个目录下，所以**它记得昨天聊过什么**。

> 前提：deep-code 的版本要带 `AGENTS.md` 读取能力。当前发布版（0.4.15）里没有，
> 所以这一版宠物行为上仍会像编程助手；人设要等带该能力的版本。

---

## 对话管理（右键菜单）

宠物右键 → 原生菜单：

| 菜单项 | 行为 |
|---|---|
| **历史对话** | 弹出独立的历史窗口（见下） |
| **新建对话** | 只丢掉"当前对话指针"，**不删任何历史文件**；下一次提问开新会话 |
| **打开对话** | 叫出聊天面板 |
| **退出** | 退出 app |

### 历史对话窗口（`history.html`）

右键 → 历史对话，宠物旁边弹出 380×500 的列表：

- 每行：**标题**（第一条用户消息，截 24 字）+ **相对时间**（刚刚 / N 分钟前 / N 天前）；
- 当前那条带蓝色边框和「当前」徽章；
- 点一条 → 切过去（写指针 + 广播），聊天窗口自动重读记录，历史窗口自己收起；
- 顶部有「＋ 新建」和「✕ 收起」；`Esc` 也能收起；
- 每次窗口获得焦点都会重读列表。

实现上有个"当前对话"指针（`current-session.json`）：

- 提问时：有当前对话 → `deepcode -p … --resume <id>`；没有 → `--new`；
- 每轮结束的 `print.result` 里带 `session_id`，生效后会回写指针；
- 面板的 `load_history` 跟随当前对话；指针存在但为空时**不会**回退到最新一条
  （否则"新建对话"看起来会失效）。

## 工作区（pet 的 cwd）

```
~/Library/Application Support/dev.deepcode.pet/
├── current-session.json          当前指向哪条对话
├── window.json                   宠物窗口位置
├── settings.json                 模型选择（get_model / set_model 读写）
└── agent/                        ← deepcode -p 的 cwd
    ├── AGENTS.md                 人格（首启自动生成，改它就改性格）
    └── .deep-code/
        ├── sessions/*.json       对话历史（一条一个文件，由压缩收敛）
        └── checkpoints/          每轮的极简工作区快照
```

想彻底清空历史：删掉 `.deep-code/sessions/`（右键菜单只做"新建"，不会删文件）。

---

## key 与模型（宠物不存任何凭证）

宠物**没有自己的 API key**，它 spawn `deepcode` 时继承你的环境与 `HOME`，
由 deepcode 自己读 `~/.deep-code/config.toml` 或 `DEEPSEEK_API_KEY`。所以：

- 宠物进程从不接触 key；计费走你同一个账号；
- 换 key 只改一处，宠物自动跟随；
- 别人要用这个宠物，就得先装好并配好 deepcode。

启动时会跑一次 `deepcode doctor --json` 做体检：key 缺失会直接告诉你，
二进制不在 PATH 里会提示"找不到 deepcode"。

模型默认 **`deepseek-flash`**（便宜、快），注入方式是用环境变量 `DEEP_CODE_MODEL`
（`deepcode -p` 没有 `--model` 参数，但 config 层认这个变量，优先级高于配置文件）。

优先级：`DEEP_CODE_PET_MODEL` > `settings.json` > 继承来的 `DEEP_CODE_MODEL` > `deepseek-flash`。

```bash
DEEP_CODE_PET_MODEL=deepseek-v4-pro pnpm app     # 临时切到 pro
```

切换模型会**重新暖一次前缀缓存**（缓存按模型分开），换完第一轮是 miss，之后照常命中。

---

## 手势与快捷键

| 操作 | 效果 |
|---|---|
| 单击 | 蹦一下（**延迟 300ms 执行**；期间来了第二击就取消它，所以双击不会先蹦） |
| 双击 | 打开聊天窗口；窗口里 `Esc` 或 `✕` 收起 |
| 悬停时出现的小按钮（💬） | 立即打开对话，零延迟 |
| 右键宠物 | 原生菜单（历史对话 / 新建 / 打开 / 退出） |
| 按住拖动 | 朝拖动方向跑，窗口跟着走；松手回原状态。跨显示器、不同缩放都正常 |
| `Cmd/Ctrl` + `-` / `=` | 缩放（0.6–1.6），窗口同步，记在 localStorage |
| `Cmd/Ctrl` + `Shift` + `R` | 新建对话（不删历史） |
| `Esc` | 收起通知卡片 / 关掉输入条 |

窗口位置在每次拖完时落盘，下次启动还原。

---

## 事件 → 状态

| 事件 | 宠物 | 文字去哪 |
|---|---|---|
| `turn.started` / `reasoning.delta` / `assistant.delta` | `running` | 聊天窗口（流式） |
| `tool.started` | `running` | 聊天窗口一行小注 |
| `approval.required` | `waiting` | 聊天窗口（**只有交互式才可能来**，见下） |
| `approval.resolved` | `running` | — |
| 回合结束且 `denied_approvals > 0` | `waiting`（3s 后回落） | 聊天窗口小注 |
| `compaction.started` | — | 聊天窗口小注 |
| `turn.completed` + 有工具调用 | `review` 1.2s → `idle` | 正文只在聊天窗口 |
| `turn.completed` + 纯对话 | `jumping` 0.84s → `idle` | 正文只在聊天窗口 |
| `turn.cancelled` | `idle` | 聊天窗口小注 |
| `error` / 进程非零退出 / `status."timeout"` | `failed` 1.22s → `idle` | 聊天窗口小注 |

宠物窗口**没有气泡**：只有精灵本体 + 一张居中的通知卡片（换宠物 / 缺 key / 出错，
4 秒自动消失）。正文一律进聊天窗口（可滚动、可选中复制）。

`waving` 不挂在事件上，而是「打招呼」这类可重复触发的一次性动作：

| 时机 | 来源 |
|---|---|
| 启动 | `setTimeout(wave, 400)` |
| 双击宠物 / 点 💬 | 打开聊天窗口之前 |
| 换了一只宠物 | 素材加载成功后 |
| 新建 / 切换对话 | `conversation-changed` |

映射代码在 `brain.mjs`（纯函数，可测）。

---

## CI 与发版

### push 之后必过的门（`.github/workflows/ci.yml`）

push 到 `main` 或开 PR 时自动跑，两个 job 都绿才算过：

| job | 跑在哪 | 内容 |
|---|---|---|
| `frontend` | ubuntu | `node --check` 全部 js/mjs → `node selftest.mjs`（45 条）→ `vite build` |
| `rust` | macos-14 | `cargo fmt --check` → `cargo clippy -D warnings` → `cargo test` → `tauri build --no-bundle` |

`--no-bundle` 只编译不打包：它能证伪"配置坏了 / 代码编不过"，又不会让每次 push 花十几分钟。

### 发版：输入版本号即可（`.github/workflows/release.yml`）

```
Actions → Release → Run workflow → 版本号填 0.0.2（不带 v）
```

它会：

1. 校验 semver、确认 tag 没被占用、只允许从 `main` 触发；
2. **把版本号同时写进四处**：`package.json`、`src-tauri/tauri.conf.json`、
   `src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`；
3. 提交 `chore(release): vX.Y.Z` + 打 tag + 推回当前分支；
4. 两个架构各打一个 dmg（`darwin-arm64` / `darwin-x64`），挂到同一个 GitHub Release。

两点必须知道：

- **未签名**：没有开发者证书时产物是未签名 dmg，Gatekeeper 会拦。用户绕过：

  ```bash
  xattr -cr /Applications/deep-code-pet.app
  ```

- 用 `GITHUB_TOKEN` 推的提交**不会**再触发 CI（GitHub 防递归）。所以发版前请确认
  `main` 上的 CI 是绿的。

### 建仓后第一次推送

```bash
cd deep-code-pet
git init -b main
git add -A
git commit -m "feat: v0.0.1"
git remote add origin <仓库地址>
git push -u origin main
```

> 仓库用 **pnpm**（`pnpm-lock.yaml` 是唯一的锁），CI 里是 `pnpm install --frozen-lockfile`。

### 为什么是多个窗口

宠物（`main`，透明无边框）与面板（`chat` / `history`，无边框、透明、圆角）是**独立窗口**。
早期版本把它们塞进一个窗口，展开时要 resize——改尺寸和改位置是两次系统调用，
中间那一帧宠物必然跳一下，换顺序也躲不掉。拆开之后展开面板**完全不碰宠物窗口**。

三个窗口都是 `decorations: false` + `transparent: true`：面板顶部那条 header 就是拖动区
（`data-tauri-drag-region`），右侧 `✕` 收起，`Cmd/Ctrl + W` 也一样；圆角由 CSS 的
`border-radius` + `overflow: hidden` 裁出来。

宠物窗口的层级在 `setup` 里抬到 `NSStatusWindowLevel(25)`（objc2 直接调
`NSWindow.setLevel:`）：`alwaysOnTop` 只给 floating(3)，而菜单栏是 24，
不抬高的话宠物贴到屏幕顶部就会被菜单栏压住。

## 已知限制（v0.0.1 有意为之）

1. **没有点击穿透**：窗口贴着精灵（+8px 边距），但那圈边距仍会吃掉点击。
2. **没有气泡**：所有文字都在聊天窗口；短暂通知用中央卡片（4 秒消失）。
3. **一轮一个进程**：每句话 spawn 一次 `deepcode -p`（约 0.1–0.3s + 模型延迟）。
   嫌慢可以换成常驻 `deepcode serve` + SSE，`brain.mjs` 不用改。
4. **审批只显示不闭环**：`-p` 模式下 deep-code 一律自动拒绝，所以
   `approval.required` 根本不会发出来；宠物用 `denied_approvals` 兜底提示。
5. **只验证了 macOS**：Windows/Linux 未测。
6. **没有宠物切换 UI**：拖入即换，或改 `~/.deep-code-pet/pets`。

---

## 文件地图

```
index.html           宠物窗口：精灵 / 悬停按钮 / 通知卡片
main.js              宠物侧逻辑：动画、拖动、换素材、把事件变成姿态
styles.css           透明背景、像素风放大
chat.html  chat.js  chat.css        聊天窗口：完整记录 + 多行输入 + 流式渲染
history.html  history.js  history.css  历史对话窗口：列表 / 切换 / 新建
pet-model.mjs        ★ 九状态 + 行号 + 逐帧时长 + 帧矩形
pet-state.mjs        状态机：循环 / 一次性状态播完回 idle / 同状态去重
pet-loader.mjs       pet.json 解析 + atlas 尺寸校验
brain.mjs            ★ NDJSON envelope → 状态 + 文字（纯 reducer，两个窗口共用）
selftest.mjs         45 条离线断言
src-tauri/           Rust：素材 IO / 扫目录 / spawn deepcode / 三个窗口 / 会话指针
```

## 第一次跑可能要动两处

Tauri 2 的 schema 在小版本间有出入，如果 `tauri dev` 报错，先看这两个地方：

1. `src-tauri/capabilities/default.json` 里若提示不认识某个权限名，删掉那一行即可；
   `core:default` 已包含事件监听。
2. `src-tauri/tauri.conf.json` 的 `app.macOSPrivateApi`（透明窗口必需）。

报错贴出来按实际版本改。
