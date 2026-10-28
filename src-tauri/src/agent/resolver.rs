use crate::{paths::home, settings::read_settings};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub(crate) const DISABLE_TOOLS_ENV: &str = "DEEP_CODE_DISABLE_TOOLS";

#[tauri::command]
/// 找到 `deepcode` 可执行文件。
///
/// GUI 启动的进程只有极短的 PATH（`/usr/bin:/bin:/usr/sbin:/sbin`），所以从
/// Finder / Dock 打开的 app 里 `Command::new("deepcode")` 会 ENOENT ——
/// 表现就是"dev 下能聊，装成 .app 后只会出错了"。
///
/// 顺序：显式配置 → PATH → 登录 shell 的 `command -v` → 常见安装位置。
/// 结果缓存一次，安装完 deepcode 后重启 app 即可。
pub(crate) fn resolve_agent_bin(app: &tauri::AppHandle) -> Option<PathBuf> {
    static CACHE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();

    CACHE
        .get_or_init(|| {
            if let Ok(raw) = std::env::var("DEEP_CODE_PET_BIN") {
                let path = PathBuf::from(raw.trim());
                if path.is_file() {
                    return Some(path);
                }
            }
            if let Some(raw) = read_settings(app)
                .deepcode
                .filter(|value| !value.trim().is_empty())
            {
                let path = PathBuf::from(raw.trim());
                if path.is_file() {
                    return Some(path);
                }
            }
            if let Some(found) = find_in_path() {
                return Some(prefer_real_binary(found));
            }
            if let Some(found) = find_via_login_shell() {
                return Some(prefer_real_binary(found));
            }
            common_install_locations()
                .into_iter()
                .find(|path| path.is_file())
                .map(prefer_real_binary)
        })
        .clone()
}

/// 平台相关的候选可执行文件名。
///
/// Windows 上 npm 装出来的是批处理 `deepcode.cmd`（PATH 里那个无扩展名的
/// `deepcode` 是给 Git Bash 用的 sh 脚本，CreateProcess 跑不了），所以按
/// 「原生 exe 优先」的顺序排。其它平台就是 `deepcode`。
pub(crate) fn agent_file_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["deepcode.exe", "deepcode.cmd", "deepcode.bat"]
    } else {
        &["deepcode"]
    }
}

/// 某个目录下所有值得一试的候选（按优先级）。
pub(crate) fn candidates_in(dir: &Path) -> Vec<PathBuf> {
    agent_file_names()
        .iter()
        .map(|name| dir.join(name))
        .collect()
}

pub(crate) fn is_batch(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
        .unwrap_or(false)
}

/// 批处理 shim 里写入口路径用的前缀：`%dp0%` 来自 npm 的 cmd-shim，`%~dp0` 来自
/// pnpm 的 binstub / Scoop / 手写脚本。两者都指「脚本自己所在的目录」，后面带反斜杠。
const SHIM_PREFIXES: [&str; 2] = ["%dp0%\\", "%~dp0\\"];

/// 从批处理 shim 里读出来的两个入口。
#[derive(Default)]
pub(crate) struct ShimTargets {
    /// `...\bin\deepcode`：真正的 node 入口。
    pub(crate) script: Option<PathBuf>,
    /// `node.exe`：node 和包管理器装在一起时，解释器就在旁边。
    pub(crate) node: Option<PathBuf>,
}

/// 解析 npm / pnpm 生成的 `.cmd` 转发脚本。
///
/// npm 的模板里有两行带 `%dp0%\`：
/// ```text
/// IF EXIST "%dp0%\node.exe" ( ... )
/// endLocal & ... & "%_prog%"  "%dp0%\node_modules\@scope\pkg\bin\deepcode" %*
/// ```
/// pnpm 的模板则是 `"%~dp0\..\pkg\bin\vite.js"`。区分办法不看路径里有没有
/// `node_modules`（pnpm 就没有），而是看**文件名**：`node.exe` 是解释器，其余存在的
/// 文件就是入口。
pub(crate) fn parse_shim(shim: &Path) -> ShimTargets {
    let Some(dir) = shim.parent() else {
        return ShimTargets::default();
    };
    let Ok(text) = fs::read_to_string(shim) else {
        return ShimTargets::default();
    };
    let mut targets = ShimTargets::default();
    for line in text.lines() {
        for prefix in SHIM_PREFIXES {
            for (at, _) in line.match_indices(prefix) {
                let tail = &line[at + prefix.len()..];
                let end = tail.find('"').unwrap_or(tail.len());
                let relative = tail[..end].trim();
                if relative.is_empty() {
                    continue;
                }
                // 批处理里的分隔符固定是反斜杠；换成 `/` 让非 Windows 上的单测也能跑。
                let candidate = dir.join(relative.replace('\\', "/"));
                if !candidate.is_file() {
                    continue;
                }
                let is_node = candidate
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.eq_ignore_ascii_case("node.exe"));
                if is_node {
                    targets.node.get_or_insert(candidate);
                } else {
                    targets.script.get_or_insert(candidate);
                }
            }
        }
    }
    targets
}

/// npm 的 `.cmd` 拿 `bin/deepcode` 这个 node 脚本当入口；同目录的 `bin/deepcode.exe`
/// 才是原生二进制 —— 有就用它，省掉一层 node。
pub(crate) fn windows_native_from_shim(shim: &Path) -> Option<PathBuf> {
    let script = parse_shim(shim).script?;
    let exe = script.with_extension("exe");
    exe.is_file().then_some(exe)
}

/// npm 包里的 `deepcode` 是 `#!/usr/bin/env node` 启动脚本，而 GUI 进程同样没有 `node`。
/// 同目录（**真实路径**的同目录）的 `deepcode-bin` 才是原生二进制 —— 用它可绕开 node。
/// Windows 的等价物是 `.cmd` 转发脚本背后的 `deepcode.exe`。
pub(crate) fn prefer_real_binary(path: PathBuf) -> PathBuf {
    if cfg!(windows) {
        // 只有批处理 shim 才需要往下钻一层；`.exe` 已经是原生二进制，别再读它一遍。
        return if is_batch(&path) {
            windows_native_from_shim(&path).unwrap_or(path)
        } else {
            path
        };
    }
    // 常见布局是 `bin/deepcode` 符号链接到 `.../lib/node_modules/<pkg>/bin/deepcode`，
    // 真正的二进制在那个真实目录里，所以要先解析链接。
    let real = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    let sibling = real.with_file_name("deepcode-bin");
    if sibling.is_file() {
        return sibling;
    }
    let direct = path.with_file_name("deepcode-bin");
    if direct.is_file() {
        return direct;
    }
    path
}

pub(crate) fn find_in_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    // 按文件名优先级找：Windows 上任何一个目录里的 `deepcode.exe` 都比 `deepcode.cmd` 可信。
    for name in agent_file_names() {
        for dir in &dirs {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Windows 上没有登录 shell；等价物是 `where`（在 System32 里，而 System32 永远在 PATH 上）。
pub(crate) fn find_via_where() -> Option<PathBuf> {
    for name in agent_file_names() {
        let mut command = Command::new("where");
        command.arg(name).stdin(Stdio::null()).stderr(Stdio::null());
        hide_console(&mut command);
        let Ok(output) = command.output() else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let candidate = PathBuf::from(line.trim());
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 走登录 shell 问一次：`fnm` / `nvm` / `homebrew` 之类只在 shell 配置里加 PATH。
pub(crate) fn find_via_login_shell() -> Option<PathBuf> {
    if cfg!(windows) {
        return find_via_where();
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    // -l 加载登录配置；-i 再加载交互配置（fnm / nvm 通常写在 .zshrc 里）。
    for flags in ["-lic", "-lc", "-ic"] {
        let output = match std::process::Command::new(&shell)
            .arg(flags)
            .arg("command -v deepcode")
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
        {
            Ok(output) => output,
            Err(_) => continue,
        };
        if !output.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let candidate = PathBuf::from(line.trim());
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 扫 `<root>/<版本目录>/<后缀>` 这种「一个 node 版本一个目录」的布局。
pub(crate) fn scan_installation_dirs(root: &Path, suffix: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        out.extend(candidates_in(&entry.path().join(suffix)));
    }
}

pub(crate) fn common_install_locations() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = home() {
        if cfg!(windows) {
            // npm / pnpm 的全局 bin —— `deepcode.cmd` 就住在这里。
            for base in [
                std::env::var_os("APPDATA").map(|dir| PathBuf::from(dir).join("npm")),
                std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("pnpm")),
                Some(home.join("scoop/shims")),
                Some(home.join(".bun/bin")),
                Some(home.join(".volta/bin")),
            ]
            .into_iter()
            .flatten()
            {
                out.extend(candidates_in(&base));
            }
            if let Some(appdata) = std::env::var_os("APPDATA") {
                let appdata = PathBuf::from(appdata);
                // fnm：%APPDATA%\fnm\node-versions\<v>\installation
                scan_installation_dirs(
                    &appdata.join("fnm/node-versions"),
                    Path::new("installation"),
                    &mut out,
                );
                // nvm-windows：%APPDATA%\nvm\<v>（可执行文件直接放在版本目录里）
                scan_installation_dirs(&appdata.join("nvm"), Path::new(""), &mut out);
            }
        } else {
            // macOS / Linux 共用的用户级安装位置。
            for base in [
                ".local/bin",
                ".local/share/pnpm",
                ".bun/bin",
                ".volta/bin",
                ".npm-global/bin",
                ".npm/bin",
                "Library/pnpm",
            ] {
                out.extend(candidates_in(&home.join(base)));
            }
            // fnm 的多 shell 目录（`~/.local/state/fnm_multishells/<id>/bin`）
            scan_installation_dirs(
                &home.join(".local/state/fnm_multishells"),
                Path::new("bin"),
                &mut out,
            );
            // fnm 的稳定安装目录（multishells 会被清理，这里是长期的）
            scan_installation_dirs(
                &home.join(".local/share/fnm/node-versions"),
                Path::new("installation/bin"),
                &mut out,
            );
            // nvm
            scan_installation_dirs(&home.join(".nvm/versions/node"), Path::new("bin"), &mut out);
        }
    }
    if cfg!(windows) {
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            out.extend(candidates_in(&PathBuf::from(program_files).join("nodejs")));
        }
    } else {
        for absolute in [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/usr/bin",
            "/snap/bin",
            "/opt/local/bin",
        ] {
            out.extend(candidates_in(Path::new(absolute)));
        }
    }
    out
}

/// Windows 上 GUI 进程 spawn 控制台程序会闪一个黑框，统一关掉。
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub(crate) fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

/// 找 `node.exe`：Windows 上 node 只以 `node.exe` 出现，GUI 进程的 PATH 又很短。
pub(crate) fn find_node_in_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("node.exe"))
        .find(|candidate| candidate.is_file())
}

/// 一个 `deepcode` 进程的启动方式。
enum Launcher {
    /// 原生可执行文件：macOS / Linux 的 `deepcode`、Windows 的 `deepcode.exe`。
    Native(PathBuf),
    /// 只找到 npm 的 `.cmd` 转发脚本时的退路：用 node 跑它背后的脚本。
    NodeScript { node: PathBuf, script: PathBuf },
}

/// 把「解析出来的路径」翻译成真的能跑的进程。
///
/// Windows 上 `CreateProcess` 不认批处理，`Command::new("…\\deepcode.cmd")` 必失败，
/// 所以这里把 `.cmd` 拆成 `deepcode.exe` 或 `node.exe <脚本>`。两条路都走不通时
/// **直接报错**，而不是退回 `cmd /C` —— cmd 会解释 `&` `%` 这类字符，用户的提问
/// 里有它们时会被静默吃掉。
fn launcher(bin: &Path) -> Result<Launcher, String> {
    if !cfg!(windows) || !is_batch(bin) {
        return Ok(Launcher::Native(bin.to_path_buf()));
    }
    let targets = parse_shim(bin);
    if let Some(script) = targets.script {
        let exe = script.with_extension("exe");
        if exe.is_file() {
            return Ok(Launcher::Native(exe));
        }
        if let Some(node) = targets.node.or_else(find_node_in_path) {
            return Ok(Launcher::NodeScript { node, script });
        }
        return Err(format!(
            "{} 是 npm 的批处理转发脚本，但它背后的 node 找不到。\
             请把 DEEP_CODE_PET_BIN 指向 `…\\node_modules\\@…\\deepcode\\bin\\deepcode.exe`，\
             或在 settings.json 里写 deepcode 的完整路径。",
            bin.display()
        ));
    }
    Err(format!(
        "{} 是个批处理脚本，但没能从里面读出真正的入口；\
         请把 DEEP_CODE_PET_BIN 指向 deepcode 的原生可执行文件。",
        bin.display()
    ))
}

pub(crate) fn agent_command(bin: &Path) -> Result<Command, String> {
    let mut command = match launcher(bin)? {
        Launcher::Native(program) => Command::new(program),
        Launcher::NodeScript { node, script } => {
            let mut command = Command::new(node);
            command.arg(script);
            command
        }
    };
    // 宠物只聊天：让 deep-code 一个工具都不挂（请求里也就没有工具 schema），
    // 而不是靠 persona 劝模型别用。老版本不认识这个变量，只会忽略。
    command.env(DISABLE_TOOLS_ENV, "1");
    hide_console(&mut command);
    Ok(command)
}

#[tauri::command]
pub(crate) fn get_agent_bin(app: tauri::AppHandle) -> Option<String> {
    resolve_agent_bin(&app).map(|path| path.display().to_string())
}
#[tauri::command]
pub(crate) fn doctor_summary(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    // 借 deep-code 自己的体检报告回答"我能不能用"：
    // key 是哪来的（file / env / missing）、用的是哪个模型、配置在哪儿。
    let bin = resolve_agent_bin(&app).ok_or_else(missing_agent_message)?;
    let output = agent_command(&bin)?
        .arg("doctor")
        .arg("--json")
        .output()
        .map_err(|error| format!("跑不起 `{}`: {error}", bin.display()))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("doctor 输出解析失败: {error}"))
}

pub(crate) fn child_path(bin: &Path) -> std::ffi::OsString {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(parent) = bin.parent() {
        dirs.push(parent.to_path_buf());
    }
    if let Some(existing) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(dirs).unwrap_or_else(|_| std::env::var_os("PATH").unwrap_or_default())
}

pub(crate) fn missing_agent_message() -> String {
    "找不到 deepcode 可执行文件。GUI 启动的 app 不继承 shell 的 PATH，请先在终端确认 \
     `deepcode --version` 能输出，然后重启本应用；或在 settings.json 里指定 deepcode 路径，\
     或设置环境变量 DEEP_CODE_PET_BIN。"
        .to_string()
}
