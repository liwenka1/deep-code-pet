use std::{fs, path::PathBuf};
use tauri::{Manager, PhysicalPosition};

/// 聊天面板放在宠物旁边：优先右边，放不下就左边，两边都放不下就夹进屏幕。
pub(crate) fn chat_placement(
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

#[tauri::command]
pub(crate) fn show_pet_menu(app: tauri::AppHandle) -> Result<(), String> {
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
pub(crate) fn open_chat(app: tauri::AppHandle) -> Result<(), String> {
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
pub(crate) fn close_chat(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(chat) = app.get_webview_window("chat") {
        chat.hide().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn open_history(app: tauri::AppHandle) -> Result<(), String> {
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
pub(crate) fn close_history(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(history) = app.get_webview_window("history") {
        history.hide().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn pos_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("window.json"))
}

pub(crate) fn write_pos(app: &tauri::AppHandle, position: PhysicalPosition<i32>) {
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
pub(crate) struct DragHandle(std::sync::Mutex<Option<(f64, f64)>>);

#[tauri::command]
pub(crate) fn drag_begin(
    window: tauri::WebviewWindow,
    state: tauri::State<DragHandle>,
) -> Result<(), String> {
    let cursor = window
        .cursor_position()
        .map_err(|error| error.to_string())?;
    let pos = window.outer_position().map_err(|error| error.to_string())?;
    let mut slot = state.0.lock().map_err(|error| error.to_string())?;
    *slot = Some((cursor.x - f64::from(pos.x), cursor.y - f64::from(pos.y)));
    Ok(())
}

#[tauri::command]
pub(crate) fn drag_update(
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
pub(crate) fn drag_end(state: tauri::State<DragHandle>) -> Result<(), String> {
    let mut slot = state.0.lock().map_err(|error| error.to_string())?;
    *slot = None;
    Ok(())
}

#[tauri::command]
pub(crate) fn set_window_layout(
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
pub(crate) fn save_window_pos(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<(), String> {
    let position = window.outer_position().map_err(|error| error.to_string())?;
    write_pos(&app, position);
    Ok(())
}

#[tauri::command]
pub(crate) fn restore_window_pos(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<(), String> {
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
pub(crate) fn raise_above_menu_bar(window: &tauri::WebviewWindow) -> Result<(), String> {
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
