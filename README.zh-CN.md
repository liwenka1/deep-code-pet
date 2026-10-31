<div align="center">
  <img src="src-tauri/icons/128x128@2x.png" alt="deep-code-pet 图标" width="96" height="96">
  <h1>deep-code-pet</h1>
  <p>
    <strong>住在你桌面上的宠物（macOS / Windows / Linux）</strong>
    <br>
    素材来自本地精灵图集，大脑是你终端里已经在跑的那个 Agent。
  </p>
  <p>
    <a href="https://github.com/liwenka1/deep-code-pet/releases">
      <img src="https://img.shields.io/github/v/release/liwenka1/deep-code-pet?style=flat-square" alt="Release">
    </a>
    <a href="LICENSE">
      <img src="https://img.shields.io/github/license/liwenka1/deep-code-pet?style=flat-square" alt="License">
    </a>
    <img src="https://img.shields.io/badge/-macOS-black?style=flat-square&logo=apple&logoColor=white" alt="macOS">
    <img src="https://img.shields.io/badge/-Windows-black?style=flat-square&logo=windows&logoColor=white" alt="Windows">
    <img src="https://img.shields.io/badge/-Linux-black?style=flat-square&logo=linux&logoColor=white" alt="Linux">
  </p>
  <p>
    <a href="https://github.com/liwenka1/deep-code-pet/releases">下载</a> •
    <a href="README.md">English</a>
  </p>
</div>

---

## 这是什么

**deep-code-pet** 是一个常驻桌面的小宠物：从本地文件夹读精灵图集，用九状态机播动画，
连接你已经装好的 `deepcode` runtime —— 同一个账号、同一份配置，不用再填 key，也不在壳子里重写 Agent。

- **本地优先**：素材、人格、对话历史都在你自己的目录里
- **不自带模型**：宠物只是连接本地的 `deepcode`，模型、凭证和 Agent 行为都由它负责
- **真的只聊天**：runtime 启动时带 `DEEP_CODE_DISABLE_TOOLS=1`，工具一个都不挂
  （读文件、执行命令、联网都没有）—— 不是写句提示词劝模型别用
- **多窗口**：宠物是透明浮层，聊天与历史是独立面板，展开面板不会挪动宠物

## 功能一览

| 功能 | 说明 |
| :-- | :-- |
| **九状态精灵动画** | idle / 跑动 / 挥手 / 跳 / 失败 / 等待 / review 与两个方向，逐帧时长按素材格式表 |
| **拖入换宠物** | 把任意宠物文件夹拖进窗口即可，选择会被记住 |
| **聊天面板** | 完整记录、多行输入、流式回复、可滚动、可选中复制 |
| **历史对话** | 独立列表窗口：切换、新建、查看；会话由 deepcode 持久化在磁盘上 |
| **Deepcode runtime** | Agent 全部由 deepcode 负责，壳子只把事件投影成聊天和动画 |
| **多屏拖拽** | 窗口位置由系统光标坐标算，跨显示器、混合缩放都正常 |
| **可压到菜单栏之上** | macOS 上把宠物窗口抬到 `NSStatusWindowLevel`，能贴到屏幕最顶部；其它平台用系统自带的 `alwaysOnTop` |
| **体积小** | 透明无边框窗口贴着精灵，除了宠物本体没有可点区域 |

## 工作机制

宠物是一个被事件流驱动的状态机：

| 事件 | 宠物 | 文字 |
| :-- | :-- | :-- |
| 回合开始 / 思考增量 / 正文增量 | `running` | 聊天面板（流式） |
| 工具开始 | `running` | 聊天面板一行小注 |
| 回合结束且有工具调用 | `review` → `idle` | 回复正文在聊天面板 |
| 回合结束且纯对话 | `jumping` → `idle` | 回复正文在聊天面板 |
| 出错 / 进程非零退出 / 超时 | `failed` → `idle` | 原因只报一次 |
| 回合取消 | `idle` | 一行小注 |
| 有调用被自动拒绝（没人能回答） | `waiting` → `idle` | 一行小注 |

`waving` 不挂在事件上：启动、被叫到、换宠物、切换对话时各挥一次。

### 手势

| 操作 | 效果 |
| :-- | :-- |
| 单击 | 蹦一下（延迟 300ms，所以双击不会先蹦） |
| 双击 / 悬停出现的小按钮 | 打开聊天面板 |
| 右键 | 原生菜单：历史对话 / 新建 / 打开 / 退出 |
| 按住拖动 | 朝拖动方向跑，窗口跟随光标 |
| `Cmd/Ctrl` + `-` / `=` | 缩放宠物（0.6–1.6×），会记住 |
| `Cmd/Ctrl` + `Shift` + `R` | 新建对话（历史保留） |
| `Esc` | 收起通知卡片 / 关闭输入 |

## 素材

一只宠物就是一个文件夹，两个文件：

```
<pet>/
├── pet.json            元数据（5 个字段）
└── spritesheet.webp    精灵图集，或 .png
```

| 项 | 值 |
| :-- | :-- |
| 帧尺寸 / 列数 | 192×208，**固定 8 列**，帧在行内左对齐 |
| v1 图集 | 1536×1872（9 行） |
| v2 图集 | 1536×2288（11 行；多出的 2 行留给消费方） |
| 下限 | 图 ≥ 256×256 |

九行状态顺序固定：`idle`、`running-right`、`running-left`、`waving`、`jumping`、`failed`、
`waiting`、`running`、`review`。每行帧数与逐帧时长属于格式的一部分，`idle` 刻意用不均匀时长做眨眼。

```json
{
  "id": "demo",
  "displayName": "Demo",
  "description": "一句话描述，可省略。",
  "spriteVersionNumber": 2,
  "spritesheetPath": "spritesheet.webp"
}
```

`spriteVersionNumber` 决定 9 行还是 11 行（缺失按 v1），其它字段一律忽略。
三种安装方式：拖进窗口、放进 `~/.deep-code-pet/pets/<名字>/`、或用 `DEEP_CODE_PET_ASSET` 指定。

## 人格

宠物的人格就是 `<配置目录>/agent/AGENTS.md`，纯 Markdown，随时可以改
（macOS 在 `~/Library/Application Support/dev.deepcode.pet/agent/AGENTS.md`，Windows 在
`%APPDATA%\dev.deepcode.pet\agent\AGENTS.md`，Linux 在
`~/.config/dev.deepcode.pet/agent/AGENTS.md`）。

第一行是归属标记：它在，默认口径升级时会跟着刷新；把它删掉，这份文件就归你了，app 再也不会动它。

## 环境要求

- macOS 10.15+ / Windows 10 及以上 / Linux（需要 WebKitGTK 4.1）；发布出来的安装包在
  GitHub 的 macOS 14、Windows、Ubuntu 22.04 runner 上构建
- Node.js 22.12+ 与 pnpm
- 编译需要 [Rust 工具链](https://rustup.rs)
- 已安装并配置好的 `deepcode`（`deepcode --version` 有输出，`deepcode doctor` 能看到 key）；
  v0.4.20+ 提供本机 runtime HTTP bridge，v0.4.18+ 才能用 `DEEP_CODE_DISABLE_TOOLS` 真正关掉工具

壳子会为当前对话启动一个 `deepcode serve --http`，通过本机 SSE 接口发送消息。Agent loop、模型、工具、
审批和 session 持久化都由 deepcode 负责，所以计费走同一账号，宠物自身不存 key。

GUI 启动的 app **不会继承 shell 的 PATH**，所以可执行文件按这个顺序解析：

1. 环境变量 `DEEP_CODE_PET_BIN`
2. app 的 `settings.json` 里的 `deepcode` 字段（macOS 在
   `~/Library/Application Support/dev.deepcode.pet/`，Windows 在 `%APPDATA%\dev.deepcode.pet\`，
   Linux 在 `~/.config/dev.deepcode.pet/`）
3. `PATH`
4. 登录 shell（`$SHELL -lic 'command -v deepcode'`）—— fnm / nvm / Homebrew 装的都能问到；
   Windows 上的等价物是 `where deepcode`
5. 常见安装位置（fnm 的 node-versions、nvm、`.local/bin`、`.bun`、`.volta`、Homebrew、
   `%APPDATA%\npm`、`%LOCALAPPDATA%\pnpm`、Scoop）

找到之后会优先用 node 脚本背后的原生二进制：npm 包里的 `deepcode` 是
`#!/usr/bin/env node` 启动脚本，而 GUI 进程同样没有 `node`。macOS / Linux 上这个二进制是同目录的
`deepcode-bin`；Windows 上 npm 装出来的是 `CreateProcess` 根本执行不了的 `deepcode.cmd`，
所以要读这个 `.cmd` 找到真实入口、优先用包内的 `bin\deepcode.exe`，找不到才用 `node.exe` 跑脚本 ——
不走 `cmd.exe`，否则你提问里的 `&`、`%` 会被它解释掉。

## 下载

从 [Releases](https://github.com/liwenka1/deep-code-pet/releases) 下载对应平台的安装包：

| 平台 | 产物 |
| :-- | :-- |
| macOS（Apple Silicon / Intel） | `.dmg` |
| Windows x64 | `-setup.exe`（NSIS） |
| Linux x64 | `.AppImage` / `.deb` |

当前构建都未签名。macOS 首次启动会拦：

```bash
xattr -cr /Applications/deep-code-pet.app
```

跑一次即可，之后再打开就正常。Windows 上 SmartScreen 会提示「Windows 已保护你的电脑」，
点「更多信息 → 仍要运行」。Linux 的产物本来就是未签名的：`.AppImage` 记得 `chmod +x`，
`.deb` 用包管理器装。

## 本地开发

```bash
pnpm install      # 装依赖
pnpm test         # Vitest：动画、协议、回合隔离、素材加载回归测试
pnpm app          # tauri dev
pnpm build        # 构建 SPA 静态资源并执行 TypeScript 检查
pnpm preview      # 预览实际嵌入安装包的静态资源（先 build）
```

浏览器回归测试使用模拟的 Tauri IPC，不会调用真实模型：

```bash
pnpm exec playwright install chromium  # 首次安装浏览器
pnpm test:e2e                         # 先执行 pnpm build
```

### 项目结构

编辑器配置在 `.vscode/settings.json`：使用项目内的 TypeScript 6 SDK，并将 Rust 工程关联到
`src-tauri/Cargo.toml`。安装依赖后，如编辑器仍使用旧诊断，可重启 TypeScript / Rust 语言服务。

前端使用 **React + TypeScript + TanStack Start（SPA）+ Vite**，异步列表查询使用 TanStack Query。
桌面端只嵌入静态资源，不启动 Start 的 Node 服务。构建时仍会预渲染文档外壳；窗口界面在客户端挂载后初始化 Tauri API。

```text
src/
├── routes/             # Start 路由及文档外壳
├── windows/            # pet / chat / history 的组件与交互
├── core/               # 不依赖 React 的动画、素材校验和聊天状态
├── platform/           # 类型化的 Tauri 命令、事件和数据契约
└── shared/             # 窗口外观和共享 hooks
src-tauri/src/
├── agent/              # deepcode 可执行文件解析与薄 runtime bridge
├── conversations.rs    # 会话索引兼容层与当前会话指针
├── pets.rs             # 本地素材读取
├── settings.rs         # 设置
└── windows.rs          # 窗口、菜单、拖拽与跨屏布局
```

`pnpm build` 先生成 `dist/client`，再通过 `scripts/desktop-build.mjs` 生成 `dist/desktop`。
三个原生窗口分别加载 `/`、`chat.html`、`history.html`；这三个物理入口共享同一个 SPA 外壳，
由路由选择界面。Tauri 的 `frontendDist` 指向 `dist/desktop`，不包含 Start 的服务端构建。
`src/routeTree.gen.ts` 由路由插件生成，修改路由后通过开发或构建命令刷新。

deepcode 与桌宠壳子的职责边界见 [`DEEPCODE_BRIDGE.md`](DEEPCODE_BRIDGE.md)。新的 Agent 能力先放进
deepcode，本仓库只消费 runtime 协议。

deepcode 统一管理运行中的回合与 session。Rust 只为当前对话启动本机 runtime，转发消息和取消请求，再把 SSE
envelope 投影成 `agent-event`。事件带有会话修订号、请求 ID、回合序号和全局事件游标；前端先订阅事件，再读取包含历史与活动回合的快照，丢弃快照已包含或过期的事件。
聊天面板提供“停止”按钮，新建或切换会话会终止旧 runtime，再按 deepcode 的 `--resume` 约定启动下一份。

Rust 侧的检查与 CI 一致：

```bash
cd src-tauri && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
```

## CI 与发版

push 到 `main` 和每个 PR 都会跑 [`.github/workflows/ci.yml`](.github/workflows/ci.yml) 里的两个 job：
`frontend (checks)`（TypeScript、Vitest、SPA 构建和 Playwright，跑在 Linux 上）与 `rust (checks)`（rustfmt、clippy
`-D warnings`、cargo test、`tauri build --no-bundle`，跑在 macOS 14 / Ubuntu / Windows 三平台上）。
三平台矩阵是「跨平台」这件事的唯一真话来源。

发版是手动触发：**Actions → Release → Run workflow**，填不带 `v` 的版本号。它会校验 semver、
要求从 `main` 触发、把版本号写进四个文件、提交打 tag，然后构建 macOS 两个架构、Windows（NSIS
安装包）与 Linux（deb + AppImage），把所有安装包上传到同一个 GitHub Release。

## 已知限制

1. **没有点击穿透**：窗口贴着精灵（外扩 8px），那圈边距仍会吃掉点击。
2. **没有气泡**：所有文字都在聊天面板；短暂通知是 4 秒后消失的卡片。
3. **每个对话一份 runtime**：壳子启动 `deepcode serve --http`，同一对话的多轮消息复用它；切换对话时按 deepcode 的 `--resume` 约定重启 bridge。
4. **没有任何工具（设计如此）**：宠物用 `DEEP_CODE_DISABLE_TOOLS=1` 起 `deepcode`，
   所以没有东西要审批、也没有东西可执行 —— 让它改代码，它会让你去找编程助手。
   （`deepcode` 0.4.18 之前的版本不认识这个变量，会退回 persona 里的文字约定。）
5. **平台差异**：「压在菜单栏之上」只有 macOS 有（其它平台是系统自带的 `alwaysOnTop`）；
   Linux 上的透明窗口需要开合成器（compositor）。
6. **没有宠物选择器**：拖入即换，或直接改 `~/.deep-code-pet/pets`。

## 许可

[MIT](LICENSE)
