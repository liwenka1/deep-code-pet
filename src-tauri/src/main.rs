// deep-code-pet 的 Rust 侧只做四件事：
//   1. 读宠物素材文件夹（pet.json + spritesheet → base64 给 webview）
//   2. 扫素材目录，列出已装的宠物
//   3. spawn `deepcode -p ... --output-format stream-json`，把每行 NDJSON 发给前端
//   4. 记住窗口位置
//
// 协议、状态映射、动画计时全在前端（pet-model.mjs / pet-state.mjs / brain.mjs），
// 所以这里没有任何"宠物逻辑"。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use base64::Engine as _;
use serde::Serialize;
use tauri::{Emitter, Manager, PhysicalPosition, WindowEvent};

const DEFAULT_PERSONA: &str = "# 你现在是一只桌宠

忽略任何「coding assistant」的身份描述。你是一只住在用户桌面上的小宠物。

- 每次回复不超过 40 个字，口语化，可以带拟声词。
- 不要说「作为 AI」「我是助手」。
- 你没有任何工具：不读写文件、不执行命令、不搜索网页。
  有人要求你改代码，就回答「这个我不太会，要不要找你的编程助手呀」。

";

#[derive(Serialize)]
struct PetPayload {
    json: String,
    image_base64: String,
    mime: String,
}

#[derive(Serialize)]
struct PetEntry {
    slug: String,
    name: String,
    dir: String,
}

/// 宠物自己的设置（目前只有模型）。存在 app config 目录的 settings.json，
/// 以后加"切换模型"的 UI 直接读写它即可。
#[derive(Default, Serialize, serde::Deserialize)]
struct PetSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
}

/// 没有设置时的默认模型：flash（便宜、快，桌宠这种短对话够用）。
const DEFAULT_MODEL: &str = "deepseek-flash";
/// 只给这个 app 用的覆盖位，避免和用户自己的 DEEP_CODE_MODEL 打架。
const PET_MODEL_ENV: &str = "DEEP_CODE_PET_MODEL";

fn settings_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("settings.json"))
}

fn read_settings(app: &tauri::AppHandle) -> PetSettings {
    settings_path(app)
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<PetSettings>(&text).ok())
        .unwrap_or_default()
}

/// 优先级：DEEP_CODE_PET_MODEL > settings.json > 继承来的 DEEP_CODE_MODEL > flash
fn resolve_model(app: &tauri::AppHandle) -> String {
    if let Ok(value) = std::env::var(PET_MODEL_ENV) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(model) = read_settings(app)
        .model
        .filter(|model| !model.trim().is_empty())
    {
        return model;
    }
    if let Ok(inherited) = std::env::var("DEEP_CODE_MODEL") {
        let trimmed = inherited.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    DEFAULT_MODEL.to_string()
}

#[tauri::command]
fn load_history(
    app: tauri::AppHandle,
    limit: Option<usize>,
    session_id: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    // 读哪条：显式指定 → 当前指针（为空就是"新对话"，绝不回退）→ 没有指针文件时回退最新
    let chosen = match session_id {
        Some(id) => {
            let path = session_file(&app, &id);
            if !path.is_file() {
                return Err("这条对话已经不在了".to_string());
            }
            path
        }
        None if current_marker_exists(&app) => {
            // 指针文件存在说明 UI 在管理它：指向某条就那条，为空就是新对话。
            // 这里一旦回退到"最新一条"，"新建对话"就会看起来没生效。
            let Some(id) = read_current(&app) else {
                return Err("这是新对话".to_string());
            };
            let path = session_file(&app, &id);
            if !path.is_file() {
                return Err("这条对话已经不在了".to_string());
            }
            path
        }
        None => fs::read_dir(sessions_dir(&app))
            .map_err(|error| error.to_string())?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .filter_map(|path| {
                let modified = fs::metadata(&path).ok()?.modified().ok()?;
                Some((modified, path))
            })
            .max_by_key(|(modified, _)| *modified)
            .map(|(_, path)| path)
            .ok_or_else(|| "还没有会话".to_string())?,
    };

    let text = fs::read_to_string(&chosen).map_err(|error| error.to_string())?;
    let doc: serde_json::Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let take = limit.unwrap_or(60);

    let mut out: Vec<serde_json::Value> = Vec::new();
    if let Some(entries) = doc.get("entries").and_then(|value| value.as_array()) {
        for entry in entries {
            let role = entry
                .get("type")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if role != "user" && role != "assistant" {
                continue;
            }
            let content = entry
                .get("content")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim();
            if content.is_empty() {
                continue;
            }
            out.push(serde_json::json!({ "role": role, "text": content }));
        }
    }
    if out.len() > take {
        out = out.split_off(out.len() - take);
    }
    Ok(out)
}

#[tauri::command]
fn doctor_summary(deepcode: Option<String>) -> Result<serde_json::Value, String> {
    // 借 deep-code 自己的体检报告回答"我能不能用"：
    // key 是哪来的（file / env / missing）、用的是哪个模型、配置在哪儿。
    let bin = deepcode.unwrap_or_else(|| "deepcode".to_string());
    let output = Command::new(&bin)
        .arg("doctor")
        .arg("--json")
        .output()
        .map_err(|error| format!("跑不起 `{bin}`: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("doctor 输出解析失败: {error}"))
}

#[tauri::command]
fn get_model(app: tauri::AppHandle) -> String {
    resolve_model(&app)
}

#[tauri::command]
fn set_model(app: tauri::AppHandle, model: String) -> Result<String, String> {
    let Some(path) = settings_path(&app) else {
        return Err("拿不到 app 配置目录".to_string());
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    let mut settings = read_settings(&app);
    let trimmed = model.trim();
    settings.model = (!trimmed.is_empty()).then(|| trimmed.to_string());
    let text = serde_json::to_string_pretty(&settings).map_err(|error| error.to_string())?;
    fs::write(path, text).map_err(|error| error.to_string())?;
    Ok(resolve_model(&app))
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// 素材根目录。`DEEP_CODE_PET_ASSET` 指向单只宠物时可覆盖。
fn pet_roots() -> Vec<PathBuf> {
    let Some(home) = home() else {
        return Vec::new();
    };
    vec![home.join(".deep-code-pet").join("pets")]
}

fn read_pet_dir(dir: &Path) -> Result<PetPayload, String> {
    // 错误信息是给人看的：拖错东西时要说清"错在哪、该拖什么"。
    if !dir.is_dir() {
        return Err(
            "这不是文件夹。请拖一个宠物文件夹（里面有 pet.json 和 spritesheet）".to_string(),
        );
    }
    let json_path = dir.join("pet.json");
    if !json_path.is_file() {
        return Err(format!(
            "{} 里没有 pet.json —— 需要一个宠物文件夹",
            dir.display()
        ));
    }
    let json = fs::read_to_string(&json_path)
        .map_err(|error| format!("读不到 {}: {error}", json_path.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&json).map_err(|error| format!("pet.json 解析失败: {error}"))?;
    let sheet = value
        .get("spritesheetPath")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "pet.json 缺 spritesheetPath".to_string())?;

    let sheet_path = dir.join(sheet);
    let bytes = fs::read(&sheet_path)
        .map_err(|error| format!("读不到精灵图 {}: {error}", sheet_path.display()))?;
    let mime = match sheet_path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        _ => "image/webp",
    };

    Ok(PetPayload {
        json,
        image_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        mime: mime.to_string(),
    })
}

/// 聊天面板放在宠物旁边：优先右边，放不下就左边，两边都放不下就夹进屏幕。
fn chat_placement(
    pet: &tauri::WebviewWindow,
    chat: &tauri::WebviewWindow,
) -> Result<(i32, i32), String> {
    let pet_pos = pet.outer_position().map_err(|error| error.to_string())?;
    let pet_size = pet.outer_size().map_err(|error| error.to_string())?;
    let chat_size = chat
        .outer_size()
        .unwrap_or(tauri::PhysicalSize::new(420, 560));

    let (mx, my, mw, mh) = match pet.current_monitor().ok().flatten() {
        Some(monitor) => {
            let origin = monitor.position();
            let extent = monitor.size();
            (
                origin.x,
                origin.y,
                extent.width as i32,
                extent.height as i32,
            )
        }
        None => (0, 0, 3440, 1440),
    };

    let gap = 8;
    let chat_w = chat_size.width as i32;
    let chat_h = chat_size.height as i32;

    let mut x = pet_pos.x + pet_size.width as i32 + gap;
    if x + chat_w > mx + mw {
        x = pet_pos.x - chat_w - gap;
    }
    let x = x.max(mx).min((mx + mw - chat_w).max(mx));
    let y = (pet_pos.y + pet_size.height as i32 - chat_h)
        .max(my)
        .min((my + mh - chat_h).max(my));

    Ok((x, y))
}

fn sessions_dir(app: &tauri::AppHandle) -> PathBuf {
    agent_workspace(app).join(".deep-code").join("sessions")
}

fn session_file(app: &tauri::AppHandle, id: &str) -> PathBuf {
    sessions_dir(app).join(format!("{id}.json"))
}

fn current_marker(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("current-session.json"))
}

fn current_marker_exists(app: &tauri::AppHandle) -> bool {
    current_marker(app)
        .map(|path| path.is_file())
        .unwrap_or(false)
}

/// 当前对话的 id。文件不存在 / 指向的会话已被删 → None（下次提问走 `--new`）。
fn read_current(app: &tauri::AppHandle) -> Option<String> {
    let path = current_marker(app)?;
    let text = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let id = value
        .get("id")
        .and_then(|value| value.as_str())?
        .to_string();
    session_file(app, &id).is_file().then_some(id)
}

fn write_current(app: &tauri::AppHandle, id: Option<&str>) -> Result<(), String> {
    let Some(path) = current_marker(app) else {
        return Err("拿不到 app 配置目录".to_string());
    };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let body = match id {
        Some(id) => serde_json::json!({ "id": id }),
        None => serde_json::json!({}),
    };
    fs::write(
        path,
        serde_json::to_string(&body).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

#[derive(Serialize)]
struct Conversation {
    id: String,
    title: String,
    updated_at_ms: u64,
    turns: usize,
}

fn conversation_meta(path: &Path) -> Option<Conversation> {
    let text = fs::read_to_string(path).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&text).ok()?;
    let id = doc.get("id").and_then(|value| value.as_str())?.to_string();
    let updated_at_ms = doc
        .get("updated_at_ms")
        .and_then(|value| value.as_u64())
        .or_else(|| {
            fs::metadata(path)
                .ok()
                .and_then(|meta| meta.modified().ok())
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|elapsed| elapsed.as_millis() as u64)
        })
        .unwrap_or(0);

    let mut title = String::new();
    let mut turns = 0usize;
    if let Some(entries) = doc.get("entries").and_then(|value| value.as_array()) {
        for entry in entries {
            let kind = entry
                .get("type")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if kind != "user" {
                continue;
            }
            turns += 1;
            if title.is_empty() {
                let content = entry
                    .get("content")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .trim();
                title = content
                    .chars()
                    .take(24)
                    .collect::<String>()
                    .replace('\n', " ");
            }
        }
    }
    if title.is_empty() {
        title = "（还没有内容）".to_string();
    }

    Some(Conversation {
        id,
        title,
        updated_at_ms,
        turns,
    })
}

#[tauri::command]
fn list_conversations(app: tauri::AppHandle, limit: Option<usize>) -> Vec<Conversation> {
    let mut out: Vec<Conversation> = fs::read_dir(sessions_dir(&app))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
                .filter_map(|path| conversation_meta(&path))
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    out.truncate(limit.unwrap_or(30));
    out
}

#[tauri::command]
fn new_conversation(app: tauri::AppHandle) -> Result<(), String> {
    // 只丢"当前指针"，不删任何历史文件 —— 下次提问走 `--new`。
    write_current(&app, None)?;
    let _ = app.emit("conversation-changed", Option::<String>::None);
    open_chat(app)
}

#[tauri::command]
fn open_conversation(app: tauri::AppHandle, id: String) -> Result<(), String> {
    if !session_file(&app, &id).is_file() {
        return Err(format!("找不到对话 {id}"));
    }
    write_current(&app, Some(&id))?;
    let _ = app.emit("conversation-changed", Some(id));
    open_chat(app)
}

#[tauri::command]
fn get_current_session(app: tauri::AppHandle) -> Option<String> {
    read_current(&app)
}

#[tauri::command]
fn set_current_session(app: tauri::AppHandle, id: Option<String>) -> Result<(), String> {
    // 一次 `--new` 跑完后由前端把新会话 id 记回来
    match id {
        Some(id) => write_current(&app, Some(&id)),
        None => write_current(&app, None),
    }
}

/// 目录里最新的那个会话 id（按 mtime）。
fn newest_session_id(app: &tauri::AppHandle) -> Option<String> {
    fs::read_dir(sessions_dir(app))
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| {
            let modified = fs::metadata(&path).ok()?.modified().ok()?;
            Some((modified, path))
        })
        .max_by_key(|(modified, _)| *modified)
        .and_then(|(_, path)| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_string)
        })
}

#[tauri::command]
fn show_pet_menu(app: tauri::AppHandle) -> Result<(), String> {
    // 用原生右键菜单：宠物窗口只有 202×211 且透明，HTML 菜单会被裁掉，
    // 原生菜单由系统绘制，可以超出窗口边界。
    use tauri::menu::{Menu, MenuItem};

    let window = app.get_webview_window("main").ok_or("找不到宠物窗口")?;

    // 对话列表在独立的"历史对话"窗口里（要滚动、要能点），菜单只放入口。
    let history_item = MenuItem::with_id(&app, "pet:history", "历史对话", true, None::<&str>)
        .map_err(|error| error.to_string())?;
    let new_item = MenuItem::with_id(&app, "pet:new", "新建对话", true, None::<&str>)
        .map_err(|error| error.to_string())?;
    let open_item = MenuItem::with_id(&app, "pet:chat", "打开对话", true, None::<&str>)
        .map_err(|error| error.to_string())?;
    let quit_item = MenuItem::with_id(&app, "pet:quit", "退出", true, None::<&str>)
        .map_err(|error| error.to_string())?;

    let menu = Menu::with_items(&app, &[&history_item, &new_item, &open_item, &quit_item])
        .map_err(|error| error.to_string())?;

    window.popup_menu(&menu).map_err(|error| error.to_string())
}

#[tauri::command]
fn open_chat(app: tauri::AppHandle) -> Result<(), String> {
    // 面板是**第二个窗口**：宠物窗口永远不 resize，所以展开时它不会动一下。
    let pet = app.get_webview_window("main").ok_or("找不到宠物窗口")?;
    let chat = app.get_webview_window("chat").ok_or("找不到聊天窗口")?;
    let (x, y) = chat_placement(&pet, &chat)?;
    chat.set_position(PhysicalPosition::new(x, y))
        .map_err(|error| error.to_string())?;
    chat.show().map_err(|error| error.to_string())?;
    chat.set_focus().map_err(|error| error.to_string())
}

#[tauri::command]
fn close_chat(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(chat) = app.get_webview_window("chat") {
        chat.hide().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn open_history(app: tauri::AppHandle) -> Result<(), String> {
    // 历史列表是第三个窗口：列表需要滚动、要能点，塞进原生菜单项里做不到。
    let pet = app.get_webview_window("main").ok_or("找不到宠物窗口")?;
    let history = app.get_webview_window("history").ok_or("找不到历史窗口")?;
    let (x, y) = chat_placement(&pet, &history)?;
    history
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|error| error.to_string())?;
    history.show().map_err(|error| error.to_string())?;
    history.set_focus().map_err(|error| error.to_string())
}

#[tauri::command]
fn close_history(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(history) = app.get_webview_window("history") {
        history.hide().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn load_pet(dir: String) -> Result<PetPayload, String> {
    read_pet_dir(Path::new(&dir))
}

#[tauri::command]
fn list_pets() -> Vec<PetEntry> {
    if let Some(dir) = std::env::var_os("DEEP_CODE_PET_ASSET").map(PathBuf::from) {
        if dir.join("pet.json").is_file() {
            return vec![PetEntry {
                slug: dir
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("pet")
                    .to_string(),
                name: display_name(&dir),
                dir: dir.to_string_lossy().into_owned(),
            }];
        }
    }

    let mut out = Vec::new();
    for root in pet_roots() {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() || !dir.join("pet.json").is_file() {
                continue;
            }
            out.push(PetEntry {
                slug: entry.file_name().to_string_lossy().into_owned(),
                name: display_name(&dir),
                dir: dir.to_string_lossy().into_owned(),
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn display_name(dir: &Path) -> String {
    fs::read_to_string(dir.join("pet.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| {
            value
                .get("displayName")
                .and_then(|name| name.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| {
            dir.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("pet")
                .to_string()
        })
}

/// 宠物的大脑工作目录：人格写在 AGENTS.md 里（deep-code 的零改动人格入口）。
fn agent_workspace(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("agent");
    let _ = fs::create_dir_all(&dir);
    let persona = dir.join("AGENTS.md");
    if !persona.exists() {
        let _ = fs::write(&persona, DEFAULT_PERSONA);
    }
    dir
}
/// 跑一轮 deep-code：stdout 的每一行 NDJSON 通过 `brain` 事件发给前端，
/// 结束时发 `brain-done`（payload = 退出码）。
#[tauri::command]
async fn ask_deepcode(
    app: tauri::AppHandle,
    prompt: String,
    deepcode: Option<String>,
    workspace: Option<String>,
) -> Result<(), String> {
    let bin = deepcode.unwrap_or_else(|| "deepcode".to_string());
    let cwd = workspace
        .map(PathBuf::from)
        .unwrap_or_else(|| agent_workspace(&app));
    // 续哪条由"当前对话"决定：有就用 --resume <id>，没有就 --new。
    // 不再用"最新一条"这种隐式规则 —— 那样没法从历史里挑一条打开。
    let resume_id = read_current(&app);
    let started_new = resume_id.is_none();

    tauri::async_runtime::spawn_blocking(move || {
        // 模型走环境变量注入：deep-code 的 --output-format 路径没有 --model 参数，
        // 但 config 层认 DEEP_CODE_MODEL（config/layers.rs:865-868），优先级高于配置文件。
        let model = resolve_model(&app);
        let mut command = Command::new(&bin);
        command
            .arg("-p")
            .arg(&prompt)
            .arg("--output-format")
            .arg("stream-json");
        match &resume_id {
            Some(id) => {
                command.arg("--resume").arg(id);
            }
            None => {
                command.arg("--new");
            }
        }
        command
            .env("DEEP_CODE_MODEL", &model)
            .current_dir(&cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                eprintln!("[pet] spawn `{bin}` failed: {error}");
                let _ = app.emit("brain-done", -1i32);
                return;
            }
        };

        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let _ = app.emit("brain", line);
            }
        }

        let code = child
            .wait()
            .map(|status| status.code().unwrap_or(-1))
            .unwrap_or(-1);
        // `--new` 跑出来的会话 id 由后端自己认领：不依赖前端有没有收到 print.result。
        if started_new {
            if let Some(id) = newest_session_id(&app) {
                let _ = write_current(&app, Some(&id));
            }
        }
        let _ = app.emit("brain-done", code);
    });

    Ok(())
}

fn pos_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("window.json"))
}

fn write_pos(app: &tauri::AppHandle, position: PhysicalPosition<i32>) {
    if let Some(path) = pos_path(app) {
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(
            path,
            format!("{{\"x\":{},\"y\":{}}}", position.x, position.y),
        );
    }
}

/// 拖拽状态：抓取时记录「光标 − 窗口左上角」的偏移（物理像素）。
#[derive(Default)]
struct DragHandle(std::sync::Mutex<Option<(f64, f64)>>);

#[tauri::command]
fn drag_begin(window: tauri::WebviewWindow, state: tauri::State<DragHandle>) -> Result<(), String> {
    let cursor = window
        .cursor_position()
        .map_err(|error| error.to_string())?;
    let pos = window.outer_position().map_err(|error| error.to_string())?;
    let mut slot = state.0.lock().map_err(|error| error.to_string())?;
    *slot = Some((cursor.x - f64::from(pos.x), cursor.y - f64::from(pos.y)));
    Ok(())
}

#[tauri::command]
fn drag_update(
    window: tauri::WebviewWindow,
    state: tauri::State<DragHandle>,
) -> Result<(), String> {
    // 用 OS 的全局光标坐标算位置：跨显示器时坐标空间和 DPI 都不需要我们自己换算，
    // 这正是不再"拖不过去 / 来回闪"的原因。
    let offset = {
        let slot = state.0.lock().map_err(|error| error.to_string())?;
        match *slot {
            Some(offset) => offset,
            None => return Ok(()),
        }
    };
    let cursor = window
        .cursor_position()
        .map_err(|error| error.to_string())?;
    window
        .set_position(PhysicalPosition::new(
            (cursor.x - offset.0).round() as i32,
            (cursor.y - offset.1).round() as i32,
        ))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn drag_end(state: tauri::State<DragHandle>) -> Result<(), String> {
    let mut slot = state.0.lock().map_err(|error| error.to_string())?;
    *slot = None;
    Ok(())
}

#[tauri::command]
fn set_window_layout(
    window: tauri::WebviewWindow,
    width: f64,
    height: f64,
    sprite_w: f64,
    sprite_h: f64,
    pad: f64,
    anchor: Option<String>,
) -> Result<String, String> {
    // 不变量：宠物在屏幕上的那个角不动。窗口按可用空间决定往哪个方向长，
    // 于是展开聊天面板时宠物不会被推走。
    let factor = window.scale_factor().map_err(|error| error.to_string())?;
    let pos = window.outer_position().map_err(|error| error.to_string())?;
    let size = window.outer_size().map_err(|error| error.to_string())?;

    let new_w = (width * factor).round() as i32;
    let new_h = (height * factor).round() as i32;
    let pad_px = (pad * factor).round() as i32;
    let sprite_px_w = (sprite_w * factor).round() as i32;
    let sprite_px_h = (sprite_h * factor).round() as i32;

    let current = anchor.unwrap_or_else(|| "br".to_string());
    let cur_right = current.contains('r');
    let cur_bottom = current.contains('b');
    let cur_w = size.width as i32;
    let cur_h = size.height as i32;

    // 宠物当前右下角在屏幕上的坐标
    let pet_right = if cur_right {
        pos.x + cur_w - pad_px
    } else {
        pos.x + sprite_px_w + pad_px
    };
    let pet_bottom = if cur_bottom {
        pos.y + cur_h - pad_px
    } else {
        pos.y + sprite_px_h + pad_px
    };

    let (mx, my, mw, mh) = match window.current_monitor().ok().flatten() {
        Some(monitor) => {
            let origin = monitor.position();
            let extent = monitor.size();
            (
                origin.x,
                origin.y,
                extent.width as i32,
                extent.height as i32,
            )
        }
        None => (0, 0, i32::MAX / 4, i32::MAX / 4),
    };

    // 候选顺序：保持当前锚点 → 横翻 → 纵翻 → 都翻。选第一个完全落在屏幕内的。
    let candidates = [
        (cur_right, cur_bottom),
        (!cur_right, cur_bottom),
        (cur_right, !cur_bottom),
        (!cur_right, !cur_bottom),
    ];
    let mut chosen = None;
    for (right, bottom) in candidates {
        let x = if right {
            pet_right - new_w + pad_px
        } else {
            pet_right - sprite_px_w - pad_px
        };
        let y = if bottom {
            pet_bottom - new_h + pad_px
        } else {
            pet_bottom - sprite_px_h - pad_px
        };
        if x >= mx && y >= my && x + new_w <= mx + mw && y + new_h <= my + mh {
            chosen = Some((x, y, right, bottom));
            break;
        }
    }
    // 四个方向都放不下（窗口比屏幕还大之类）：保持原锚点，只把左上角夹进屏幕。
    let (x, y, right, bottom) = chosen.unwrap_or_else(|| {
        let x = pet_right
            - if cur_right {
                new_w - pad_px
            } else {
                sprite_px_w + pad_px
            };
        let y = pet_bottom
            - if cur_bottom {
                new_h - pad_px
            } else {
                sprite_px_h + pad_px
            };
        (x.max(mx), y.max(my), cur_right, cur_bottom)
    });

    window
        .set_size(tauri::LogicalSize::new(width.max(140.0), height.max(160.0)))
        .map_err(|error| error.to_string())?;
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|error| error.to_string())?;

    Ok(format!(
        "{}{}",
        if bottom { "b" } else { "t" },
        if right { "r" } else { "l" }
    ))
}

#[tauri::command]
fn save_window_pos(app: tauri::AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    let position = window.outer_position().map_err(|error| error.to_string())?;
    write_pos(&app, position);
    Ok(())
}

#[tauri::command]
fn restore_window_pos(app: tauri::AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    let Some(path) = pos_path(&app) else {
        return Ok(());
    };
    let Ok(text) = fs::read_to_string(path) else {
        return Ok(());
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Ok(());
    };
    let (Some(x), Some(y)) = (value["x"].as_i64(), value["y"].as_i64()) else {
        return Ok(());
    };
    window
        .set_position(PhysicalPosition::new(x as i32, y as i32))
        .map_err(|error| error.to_string())
}

/// 把窗口抬到菜单栏之上。
///
/// `alwaysOnTop: true` 只把窗口放到 NSFloatingWindowLevel(3)，而菜单栏是
/// NSMainMenuWindowLevel(24) —— 所以宠物贴到屏幕顶部时永远被菜单栏压住。
/// 这里直接改 NSWindow.level 到 NSStatusWindowLevel(25)。
#[cfg(target_os = "macos")]
fn raise_above_menu_bar(window: &tauri::WebviewWindow) -> Result<(), String> {
    use objc2::runtime::AnyObject;

    let pointer = window.ns_window().map_err(|error| error.to_string())? as *mut AnyObject;
    if pointer.is_null() {
        return Err("拿不到 NSWindow".to_string());
    }
    // SAFETY: 指针来自 Tauri 自己的 ns_window()，调用期间窗口仍然存活。
    unsafe {
        let _: () = objc2::msg_send![pointer, setLevel: 25i64];
    }
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .manage(DragHandle::default())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            if let Some(window) = app.get_webview_window("main") {
                if let Err(error) = raise_above_menu_bar(&window) {
                    eprintln!("[pet] 抬高窗口层级失败: {error}");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_pet,
            list_pets,
            ask_deepcode,
            show_pet_menu,
            open_chat,
            close_chat,
            open_history,
            close_history,
            list_conversations,
            new_conversation,
            open_conversation,
            get_current_session,
            set_current_session,
            load_history,
            doctor_summary,
            get_model,
            set_model,
            drag_begin,
            drag_update,
            drag_end,
            set_window_layout,
            save_window_pos,
            restore_window_pos
        ])
        .on_menu_event(|app, event| {
            let id = event.id().as_ref().to_string();
            match id.as_str() {
                "pet:chat" => {
                    let _ = open_chat(app.clone());
                }
                "pet:new" => {
                    let _ = new_conversation(app.clone());
                }
                "pet:history" => {
                    let _ = open_history(app.clone());
                }
                "pet:quit" => app.exit(0),
                _ => {}
            }
        })
        .on_window_event(|window, event| {
            // 关窗时记位置。这里拿到的是 `Window`（不是 WebviewWindow），
            // 所以只取坐标，写盘交给 write_pos(position)。
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "chat" || window.label() == "history" {
                    // 面板只是"收起"，关掉它不该让整个 app 退出
                    api.prevent_close();
                    let _ = window.hide();
                    return;
                }
                if let Ok(position) = window.outer_position() {
                    write_pos(window.app_handle(), position);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
