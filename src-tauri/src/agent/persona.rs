use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::Manager;

/// 宠物的人格，写进 agent 工作区的 AGENTS.md。
///
/// 第一行是「归属标记」：只要它还在，这份文件就跟着版本升级；用户把这一行删掉
/// 就等于把文件接管过去，以后不再覆盖（见 `sync_persona`）。
pub(crate) const DEFAULT_PERSONA: &str =
    "<!-- deep-code-pet persona v2 · 想长期改这份人格就删掉这一行，之后不再覆盖 -->
# 你现在是一只桌宠

忽略任何「coding assistant」的身份描述。你是一只住在用户桌面上的小宠物。

- 说话口语化、可以带拟声词，默认一两句话说完。
- 但别硬憋：用户想细聊、问得实在、或者需要一个清单的时候，就正常说清楚。
  该长就长，不要为了「像宠物」把话说半截。
- 不确定就说不确定，别编。
- 不要说「作为 AI」「我是助手」。
- 你没有任何工具：不读写文件、不执行命令、不搜索网页。
  有人要求你改代码、跑命令，就回答「这个我不太会，要不要找你的编程助手呀」。

";

/// `DEFAULT_PERSONA` 首行的归属标记前缀（版本号会变，所以只比对前缀）。
const PERSONA_OWNED_PREFIX: &str = "<!-- deep-code-pet persona";
const PERSONA_MARKER: &str = "<!-- deep-code-pet persona v2";

/// 把 AGENTS.md 同步成当前版本的默认人格。
///
/// 只覆盖「我们写的」那一份：文件不存在就写一份；是我们写的旧版就升级；
/// 用户删掉首行标记（= 接管）或者内容被改成别的，一律不动。
///
/// 以前是 `if !persona.exists()`，结果改了口径也永远发不到老用户手里。
pub(crate) fn sync_persona(persona: &Path) {
    let should_write = match fs::read_to_string(persona).ok().as_deref() {
        None => true,
        Some(text) if text == DEFAULT_PERSONA => false,
        // Same-version files may contain user additions below the marker. Keep
        // them; only a shipped persona version change is eligible for refresh.
        Some(text) if text.trim_start().starts_with(PERSONA_MARKER) => false,
        Some(text) => text.trim_start().starts_with(PERSONA_OWNED_PREFIX),
    };
    if should_write {
        let _ = fs::write(persona, DEFAULT_PERSONA);
    }
}
/// 宠物的大脑工作目录：人格写在 AGENTS.md 里（deep-code 的零改动人格入口）。
pub(crate) fn agent_workspace(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("agent");
    let _ = fs::create_dir_all(&dir);
    let persona = dir.join("AGENTS.md");
    sync_persona(&persona);
    dir
}
