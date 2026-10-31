use serde::Serialize;
use std::{fs, path::PathBuf};
use tauri::Manager;

/// 仅保存桌宠桥接层需要的本地设置。Agent、模型和会话配置由 deepcode 自己管理。
#[derive(Default, Serialize, serde::Deserialize)]
pub(crate) struct PetSettings {
    /// 可选：显式指定 deepcode 可执行文件路径（GUI 启动时 PATH 很短）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) deepcode: Option<String>,
}

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
