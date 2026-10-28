#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent;
mod conversations;
mod paths;
mod pets;
mod settings;
mod windows;

use tauri::{Manager, WindowEvent};

fn main() {
    tauri::Builder::default()
        .manage(windows::DragHandle::default())
        .manage(agent::runtime::AgentRuntime::default())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            {
                if let Some(window) = app.get_webview_window("main") {
                    if let Err(error) = windows::raise_above_menu_bar(&window) {
                        eprintln!("[pet] 抬高窗口层级失败: {error}");
                    }
                }
            }
            // 非 macOS 上没有任何 setup 动作，`app` 于是成了未使用的参数；
            // CI 用 `clippy --all-targets -- -D warnings` 跑，必须显式消费掉。
            #[cfg(not(target_os = "macos"))]
            let _ = app;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pets::load_pet,
            pets::list_pets,
            agent::runtime::ask_deepcode,
            agent::runtime::cancel_turn,
            agent::runtime::conversation_state,
            windows::show_pet_menu,
            windows::open_chat,
            windows::close_chat,
            windows::open_history,
            windows::close_history,
            conversations::list_conversations,
            conversations::new_conversation,
            conversations::open_conversation,
            conversations::get_current_session,
            conversations::load_history,
            agent::resolver::doctor_summary,
            agent::resolver::get_agent_bin,
            settings::get_model,
            settings::set_model,
            windows::drag_begin,
            windows::drag_update,
            windows::drag_end,
            windows::set_window_layout,
            windows::save_window_pos,
            windows::restore_window_pos
        ])
        .on_menu_event(|app, event| {
            let id = event.id().as_ref().to_string();
            match id.as_str() {
                "pet:chat" => {
                    let _ = windows::open_chat(app.clone());
                }
                "pet:new" => {
                    let _ = conversations::new_conversation(app.clone());
                }
                "pet:history" => {
                    let _ = windows::open_history(app.clone());
                }
                "pet:quit" => {
                    let _ = agent::runtime::stop(app, None);
                    app.exit(0);
                }
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
                let _ = agent::runtime::stop(window.app_handle(), None);
                if let Ok(position) = window.outer_position() {
                    windows::write_pos(window.app_handle(), position);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests;
