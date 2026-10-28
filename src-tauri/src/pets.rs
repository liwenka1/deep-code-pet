use crate::paths::home;
use base64::Engine as _;
use serde::Serialize;
use std::path::PathBuf;
use std::{fs, path::Path};

#[derive(Serialize)]
pub(crate) struct PetPayload {
    json: String,
    image_base64: String,
    mime: String,
}

#[derive(Serialize)]
pub(crate) struct PetEntry {
    slug: String,
    name: String,
    dir: String,
}

/// 素材根目录。`DEEP_CODE_PET_ASSET` 指向单只宠物时可覆盖。
pub(crate) fn pet_roots() -> Vec<PathBuf> {
    let Some(home) = home() else {
        return Vec::new();
    };
    vec![home.join(".deep-code-pet").join("pets")]
}

pub(crate) fn read_pet_dir(dir: &Path) -> Result<PetPayload, String> {
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
#[tauri::command]
pub(crate) fn load_pet(dir: String) -> Result<PetPayload, String> {
    read_pet_dir(Path::new(&dir))
}

#[tauri::command]
pub(crate) fn list_pets() -> Vec<PetEntry> {
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
    // 名字升序。同样避开 `unnecessary_sort_by`：键是 String，这里直接 clone
    // （最多几十只宠物，成本可以忽略）。
    out.sort_by_key(|entry| entry.name.clone());
    out
}

pub(crate) fn display_name(dir: &Path) -> String {
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
