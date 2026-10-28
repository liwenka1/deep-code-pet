use crate::agent::{
    persona::{sync_persona, DEFAULT_PERSONA},
    resolver::{
        agent_command, parse_shim, prefer_real_binary, windows_native_from_shim, DISABLE_TOOLS_ENV,
    },
};
use std::fs;
use std::path::PathBuf;

/// 每个用例一个独立的临时目录，避免并行跑测试时互相踩。
fn temp_root(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pet-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

/// npm 在 Windows 上生成的 `deepcode.cmd`（关键行都在）。
const NPM_SHIM: &str = "\
@ECHO off
GOTO start
:find_dp0
SET dp0=%~dp0
EXIT /b
:start
SETLOCAL
CALL :find_dp0

IF EXIST \"%dp0%\\node.exe\" (
  SET \"_prog=%dp0%\\node.exe\"
) ELSE (
  SET \"_prog=node\"
  SET PATHEXT=%PATHEXT:;.JS;=;%
)

endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  \"%dp0%\\node_modules\\@scope\\deepcode\\bin\\deepcode\" %*
";

/// pnpm 生成的 binstub（照本仓库 `node_modules/.bin/*.CMD` 的形态写的）：
/// 用 `%~dp0`，入口是 `..\pkg\bin\...`，路径里没有 `node_modules`。
const PNPM_SHIM: &str = "\
@SETLOCAL
@IF NOT DEFINED NODE_PATH (
  @SET \"NODE_PATH=%~dp0\\..\\pkg\\bin;%~dp0\\..\\pkg\"
) ELSE (
  @SET \"NODE_PATH=%~dp0\\..\\pkg\\bin;%NODE_PATH%\"
)
@IF EXIST \"%~dp0\\node.exe\" (
  \"%~dp0\\node.exe\"  \"%~dp0\\..\\pkg\\bin\\deepcode\" %*
) ELSE (
  @SET PATHEXT=%PATHEXT:;.JS;=;%
  node  \"%~dp0\\..\\pkg\\bin\\deepcode\" %*
)
";

/// Windows 上的 `deepcode` 是 npm 的批处理转发脚本，入口藏在 `%dp0%\node_modules\...`；
/// `IF EXIST "%dp0%\node.exe"` 那一行不是入口。
#[test]
fn reads_npm_shim_entry_point() {
    let root = temp_root("shim");
    let shim = root.join("deepcode.cmd");
    fs::write(&shim, NPM_SHIM).unwrap();
    let package_bin = root.join("node_modules/@scope/deepcode/bin");
    fs::create_dir_all(&package_bin).unwrap();

    // 入口脚本还没落盘（shim 过期）时不该认它 —— 交给上层报一句人能看懂的话，
    // 而不是让 node 去报 `Cannot find module`。
    assert_eq!(parse_shim(&shim).script, None);

    fs::write(package_bin.join("deepcode"), "#!/usr/bin/env node\n").unwrap();
    let parsed = parse_shim(&shim);
    assert_eq!(parsed.script, Some(package_bin.join("deepcode")));
    // 测试目录里没有 node.exe，不该认它
    assert_eq!(parsed.node, None);
    // 脚本旁边没有 .exe 时不要乱改路径
    assert_eq!(windows_native_from_shim(&shim), None);

    // 出现原生 exe 就优先用它（省掉一层 node）
    fs::write(package_bin.join("deepcode.exe"), "MZ").unwrap();
    assert_eq!(
        windows_native_from_shim(&shim),
        Some(package_bin.join("deepcode.exe"))
    );

    fs::remove_dir_all(&root).ok();
}

/// pnpm 的 binstub 用 `%~dp0`；不认这个前缀的话，pnpm 装的 deepcode 一律起不来。
#[test]
fn reads_pnpm_shim_entry_point() {
    let root = temp_root("shim-pnpm");
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let shim = bin.join("deepcode.CMD");
    fs::write(&shim, PNPM_SHIM).unwrap();

    // `%~dp0\..\pkg\bin\deepcode` = `bin/../pkg/bin/deepcode`
    let script = root.join("bin/../pkg/bin/deepcode");
    fs::create_dir_all(root.join("pkg/bin")).unwrap();
    fs::write(root.join("pkg/bin/deepcode"), "#!/usr/bin/env node\n").unwrap();

    assert_eq!(parse_shim(&shim).script, Some(script));

    fs::write(root.join("pkg/bin/deepcode.exe"), "MZ").unwrap();
    assert_eq!(
        windows_native_from_shim(&shim),
        Some(root.join("bin/../pkg/bin/deepcode.exe"))
    );

    fs::remove_dir_all(&root).ok();
}

/// node 和 npm 装在一起时，`node.exe` 就在 `.cmd` 旁边 —— 退路要用到它。
#[test]
fn reads_node_next_to_shim() {
    let root = temp_root("shim-node");
    let shim = root.join("deepcode.cmd");
    fs::write(&shim, NPM_SHIM).unwrap();
    fs::write(root.join("node.exe"), "MZ").unwrap();

    assert_eq!(parse_shim(&shim).node, Some(root.join("node.exe")));
    fs::remove_dir_all(&root).ok();
}

/// 没有 `deepcode-bin` 时不要乱改路径（例如用户显式指定了别的可执行文件）。
#[test]
fn keeps_plain_binary_untouched() {
    let root = temp_root("plain");
    let binary = root.join("deepcode");
    fs::write(&binary, "native").unwrap();

    assert_eq!(prefer_real_binary(binary.clone()), binary);
    fs::remove_dir_all(&root).ok();
}

/// 宠物只聊天：每条 deep-code 进程都必须带「不要工具」这个环境变量。
#[test]
fn agent_processes_run_without_tools() {
    let command = agent_command(std::path::Path::new("deepcode")).unwrap();
    let value = command
        .get_envs()
        .find(|(key, _)| key.to_string_lossy() == DISABLE_TOOLS_ENV)
        .and_then(|(_, value)| value.map(|value| value.to_string_lossy().to_string()));
    assert_eq!(value.as_deref(), Some("1"));
}

/// 人格文件只覆盖「我们写的」那份：没有就写、旧版就升级、被用户接管就不动。
#[test]
fn syncs_owned_persona_only() {
    let root = temp_root("persona");
    let persona = root.join("AGENTS.md");

    // 1. 不存在 → 写一份
    sync_persona(&persona);
    assert_eq!(fs::read_to_string(&persona).unwrap(), DEFAULT_PERSONA);

    // 2. 是我们写的旧版（带标记、内容不同）→ 升级
    fs::write(
        &persona,
        "<!-- deep-code-pet persona v1 -->\n老的 40 字口径\n",
    )
    .unwrap();
    sync_persona(&persona);
    assert_eq!(fs::read_to_string(&persona).unwrap(), DEFAULT_PERSONA);

    // 3. 用户删掉首行标记 = 接管 → 原样保留
    fs::write(&persona, "# 我自己的宠物人格\n\n随便说。\n").unwrap();
    sync_persona(&persona);
    assert_eq!(
        fs::read_to_string(&persona).unwrap(),
        "# 我自己的宠物人格\n\n随便说。\n"
    );

    fs::remove_dir_all(&root).ok();
}

/// 常见布局：`bin/deepcode` 是指向 `lib/node_modules/<pkg>/bin/deepcode` 的符号链接，
/// 真正的二进制 `deepcode-bin` 在链接**目标**旁边。解析必须穿过链接。
/// （Windows 上建符号链接要开发者模式或管理员权限，所以这条只在 unix 跑。）
#[cfg(unix)]
#[test]
fn prefers_real_binary_behind_symlink() {
    let root = temp_root("symlink");
    let package_bin = root.join("lib/node_modules/pkg/bin");
    fs::create_dir_all(&package_bin).unwrap();
    fs::write(package_bin.join("deepcode"), "#!/usr/bin/env node\n").unwrap();
    fs::write(package_bin.join("deepcode-bin"), "native").unwrap();

    let shim_dir = root.join("bin");
    fs::create_dir_all(&shim_dir).unwrap();
    let shim = shim_dir.join("deepcode");
    std::os::unix::fs::symlink("../lib/node_modules/pkg/bin/deepcode", &shim).unwrap();

    let resolved = prefer_real_binary(shim);
    assert_eq!(
        resolved.file_name().and_then(|name| name.to_str()),
        Some("deepcode-bin")
    );
    fs::remove_dir_all(&root).ok();
}
