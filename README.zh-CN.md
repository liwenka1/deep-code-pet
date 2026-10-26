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
说话则通过你已经装好的 `deepcode` —— 同一个账号、同一份配置，不用再填 key。

- **本地优先**：素材、人格、对话历史都在你自己的目录里
- **不自带模型**：宠物只是 spawn 本地的 `deepcode`，自身不存任何凭证
- **多窗口**：宠物是透明浮层，聊天与历史是独立面板，展开面板不会挪动宠物

## 功能一览

| 功能 | 说明 |
| :-- | :-- |
| **九状态精灵动画** | idle / 跑动 / 挥手 / 跳 / 失败 / 等待 / review 与两个方向，逐帧时长按素材格式表 |
| **拖入换宠物** | 把任意宠物文件夹拖进窗口即可，选择会被记住 |
| **聊天面板** | 完整记录、多行输入、流式回复、可滚动、可选中复制 |
| **历史对话** | 独立列表窗口：切换、新建、查看；每个会话就是磁盘上的一个文件 |
| **真实 Agent** | 工具调用、出错、回合结束都会驱动动画；文字进聊天面板 |
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

## 环境要求

- macOS 10.15+ / Windows 10 及以上 / Linux（需要 WebKitGTK 4.1）；发布出来的安装包在
  GitHub 的 macOS 14、Windows、Ubuntu 22.04 runner 上构建
- Node.js 20+ 与 pnpm
- 编译需要 [Rust 工具链](https://rustup.rs)
- 已安装并配置好的 `deepcode`（`deepcode --version` 有输出，`deepcode doctor` 能看到 key）

宠物会 spawn `deepcode -p <prompt> --output-format stream-json` 并继承你的环境，
所以计费走同一账号，宠物自身不存 key。

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
pnpm test         # 离线自测（45 条断言，不需要网络与 Tauri）
pnpm app          # tauri dev
pnpm build        # 只构建前端
```

Rust 侧的检查与 CI 一致：

```bash
cd src-tauri && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
```

## CI 与发版

push 到 `main` 和每个 PR 都会跑 [`.github/workflows/ci.yml`](.github/workflows/ci.yml) 里的两个 job：
`frontend (checks)`（语法、自测、Vite 构建，跑在 Linux 上）与 `rust (checks)`（rustfmt、clippy
`-D warnings`、cargo test、`tauri build --no-bundle`，跑在 macOS 14 / Ubuntu / Windows 三平台上）。
三平台矩阵是「跨平台」这件事的唯一真话来源。

发版是手动触发：**Actions → Release → Run workflow**，填不带 `v` 的版本号。它会校验 semver、
要求从 `main` 触发、把版本号写进四个文件、提交打 tag，然后构建 macOS 两个架构、Windows（NSIS
安装包）与 Linux（deb + AppImage），把所有安装包上传到同一个 GitHub Release。

## 已知限制

1. **没有点击穿透**：窗口贴着精灵（外扩 8px），那圈边距仍会吃掉点击。
2. **没有气泡**：所有文字都在聊天面板；短暂通知是 4 秒后消失的卡片。
3. **一轮一个进程**：每句话 spawn 一次 `deepcode -p`（约 0.1–0.3s 加模型延迟）。
   换成常驻 `deepcode serve` + SSE 的话，reducer 不用改。
4. **审批无法回答**：无头模式一律自动拒绝，`approval.required` 不会发出来；
   宠物用 `denied_approvals` 兜底提示。
5. **平台差异**：「压在菜单栏之上」只有 macOS 有（其它平台是系统自带的 `alwaysOnTop`）；
   Linux 上的透明窗口需要开合成器（compositor）。
6. **没有宠物选择器**：拖入即换，或直接改 `~/.deep-code-pet/pets`。

## 许可

[MIT](LICENSE)
