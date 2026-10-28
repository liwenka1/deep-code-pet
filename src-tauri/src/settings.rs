use serde::Serialize;
use std::{fs, path::PathBuf};
use tauri::Manager;

/// 宠物自己的设置（目前只有模型）。存在 app config 目录的 settings.json，
/// 以后加"切换模型"的 UI 直接读写它即可。
#[derive(Default, Serialize, serde::Deserialize)]
pub(crate) struct PetSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) model: Option<String>,
    /// 可选：显式指定 deepcode 可执行文件路径（GUI 启动时 PATH 很短）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) deepcode: Option<String>,
}

/// 没有设置时的默认模型：flash（便宜、快，桌宠这种短对话够用）。
const DEFAULT_MODEL: &str = "deepseek-flash";
/// 只给这个 app 用的覆盖位，避免和用户自己的 DEEP_CODE_MODEL 打架。
const PET_MODEL_ENV: &str = "DEEP_CODE_PET_MODEL";

pub(crate) fn settings_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("settings.json"))
}

pub(crate) fn read_settings(app: &tauri::AppHandle) -> PetSettings {
    settings_path(app)
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<PetSettings>(&text).ok())
        .unwrap_or_default()
}

/// 优先级：DEEP_CODE_PET_MODEL > settings.json > 继承来的 DEEP_CODE_MODEL > flash
pub(crate) fn resolve_model(app: &tauri::AppHandle) -> String {
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
pub(crate) fn get_model(app: tauri::AppHandle) -> String {
    resolve_model(&app)
}

#[tauri::command]
pub(crate) fn set_model(app: tauri::AppHandle, model: String) -> Result<String, String> {
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
