use crate::{
    agent::resolver::{agent_command, child_path, missing_agent_message, resolve_agent_bin},
    agent::{persona::agent_workspace, runtime::select_conversation},
    windows::open_chat,
};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::Manager;

#[tauri::command]
pub(crate) fn load_history(
    app: tauri::AppHandle,
    limit: Option<usize>,
    session_id: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    // 读哪条：显式指定 → 当前指针（为空就是"新对话"，绝不回退）→ 没有指针文件时回退最新。
    // 会话正文通过 deepcode 的公开 export 命令读取，壳子不再依赖 session JSON schema。
    let id = match session_id {
        Some(id) => {
            validate_session_id(&id)?;
            id
        }
        None if current_marker_exists(&app) => {
            // 指针文件存在说明 UI 在管理它：指向某条就那条，为空就是新对话。
            // 这里一旦回退到"最新一条"，"新建对话"就会看起来没生效。
            let Some(id) = read_current(&app) else {
                return Ok(Vec::new());
            };
            id
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
            .and_then(|(_, path)| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(str::to_owned)
            })
            .ok_or_else(|| "还没有会话".to_string())?,
    };
    let doc = export_session(&app, &id)?;
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

pub(crate) fn export_session(
    app: &tauri::AppHandle,
    id: &str,
) -> Result<serde_json::Value, String> {
    let bin = resolve_agent_bin(app).ok_or_else(missing_agent_message)?;
    let mut command = agent_command(&bin)?;
    let output = command
        .args(["session", "export", id])
        .env("PATH", child_path(&bin))
        .current_dir(agent_workspace(app))
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("导出 deepcode 对话失败：{error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "这条对话已经不在了".to_string()
        } else {
            format!("导出 deepcode 对话失败：{stderr}")
        });
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("deepcode 对话导出解析失败：{error}"))
}

pub(crate) fn verify_session_persisted(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    let mut last_error = None;
    for attempt in 0..3 {
        match export_session(app, id) {
            Ok(document)
                if document.get("id").and_then(|value| value.as_str()) == Some(id)
                    && document
                        .get("entries")
                        .and_then(|value| value.as_array())
                        .is_some() =>
            {
                return Ok(())
            }
            Ok(_) => last_error = Some("deepcode 导出的 session 内容不完整".to_string()),
            Err(error) => last_error = Some(error),
        }
        if attempt < 2 {
            std::thread::sleep(std::time::Duration::from_millis(80));
        }
    }
    Err(last_error.unwrap_or_else(|| "deepcode session 尚未持久化".to_string()))
}
pub(crate) fn sessions_dir(app: &tauri::AppHandle) -> PathBuf {
    agent_workspace(app).join(".deep-code").join("sessions")
}

pub(crate) fn session_file(app: &tauri::AppHandle, id: &str) -> Result<PathBuf, String> {
    validate_session_id(id)?;
    Ok(sessions_dir(app).join(format!("{id}.json")))
}

fn validate_session_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 200
        || id == "."
        || id == ".."
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
    {
        return Err("无效的会话标识".into());
    }
    Ok(())
}

pub(crate) fn initialize_current(app: &tauri::AppHandle) -> Result<Option<String>, String> {
    if current_marker_exists(app) {
        return Ok(read_current(app));
    }
    let id = list_conversations(app.clone(), Some(1))
        .first()
        .map(|entry| entry.id.clone());
    write_current(app, id.as_deref())?;
    Ok(id)
}

pub(crate) fn current_marker(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("current-session.json"))
}

pub(crate) fn current_marker_exists(app: &tauri::AppHandle) -> bool {
    current_marker(app)
        .map(|path| path.is_file())
        .unwrap_or(false)
}

/// 当前对话的 id。文件不存在 / 指向的会话已被删 → None（下次提问走 `--new`）。
pub(crate) fn read_current(app: &tauri::AppHandle) -> Option<String> {
    let path = current_marker(app)?;
    let text = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let id = value
        .get("id")
        .and_then(|value| value.as_str())?
        .to_string();
    session_file(app, &id).ok()?.is_file().then_some(id)
}

pub(crate) fn write_current(app: &tauri::AppHandle, id: Option<&str>) -> Result<(), String> {
    if let Some(id) = id {
        validate_session_id(id)?;
    }
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
pub(crate) struct Conversation {
    id: String,
    title: String,
    updated_at_ms: u64,
    turns: usize,
}

pub(crate) fn conversation_meta(path: &Path) -> Option<Conversation> {
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
pub(crate) fn list_conversations(app: tauri::AppHandle, limit: Option<usize>) -> Vec<Conversation> {
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
    // 新的在前；用 sort_by_key 而不是 sort_by，避免新版 clippy 的
    // `unnecessary_sort_by` 报警（CI 的 stable 比本地工具链新）。
    out.sort_by_key(|entry| std::cmp::Reverse(entry.updated_at_ms));
    out.truncate(limit.unwrap_or(30));
    out
}

#[tauri::command]
pub(crate) fn new_conversation(app: tauri::AppHandle) -> Result<(), String> {
    // 只丢"当前指针"，不删任何历史文件 —— 下次提问走 `--new`。
    select_conversation(&app, None)?;
    open_chat(app)
}

#[tauri::command]
pub(crate) fn open_conversation(app: tauri::AppHandle, id: String) -> Result<(), String> {
    if !session_file(&app, &id)?.is_file() {
        return Err(format!("找不到对话 {id}"));
    }
    select_conversation(&app, Some(&id))?;
    open_chat(app)
}

#[tauri::command]
pub(crate) fn get_current_session(app: tauri::AppHandle) -> Option<String> {
    read_current(&app)
}

#[cfg(test)]
mod tests {
    use super::validate_session_id;
    #[test]
    fn accepts_session_ids_but_rejects_paths() {
        for id in ["session_123-abc", "abc.def"] {
            assert!(validate_session_id(id).is_ok());
        }
        for id in ["", ".", "..", "../secret", "a/b", "a\\b", "/tmp/a"] {
            assert!(validate_session_id(id).is_err());
        }
    }
}
