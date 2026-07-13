// Prevents additional console window on Windows in release, do not remove.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use tauri::State;
use tauri::{Emitter, Manager, WindowEvent};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri_plugin_notification::NotificationExt;
use vf_core::{DictEntry, HistoryEntry, InsightsSummary, Settings, Store, EngineEvent};
use vf_engine::EngineHandle;
use vf_store::SqliteStore;

pub struct AppSettings(pub std::sync::Mutex<Settings>);

#[tauri::command]
fn get_settings(settings_state: State<'_, AppSettings>) -> Result<Settings, String> {
    Ok(settings_state.0.lock().unwrap().clone())
}

#[tauri::command]
fn save_settings(
    settings: Settings,
    settings_state: State<'_, AppSettings>,
    _store: State<'_, Arc<SqliteStore>>,
    engine: State<'_, EngineHandle>,
) -> Result<(), String> {
    let settings_path = vf_store::get_default_settings_path()
        .ok_or_else(|| "Could not resolve settings path".to_string())?;
    vf_store::save_settings(&settings, &settings_path)
        .map_err(|e| e.to_string())?;
    
    // Update cached settings in Tauri state
    *settings_state.0.lock().unwrap() = settings.clone();
    
    // Apply changes to the engine
    engine
        .send(vf_core::EngineCmd::ApplySettings(Box::new(settings)))
        .map_err(|e| e.to_string())?;
    
    Ok(())
}

#[tauri::command]
async fn list_groq_models(settings_state: State<'_, AppSettings>) -> Result<Vec<String>, String> {
    let api_key = {
        let settings = settings_state.0.lock().unwrap();
        settings.llm.api_key.clone()
    };
    
    if api_key.trim().is_empty() {
        return Err("LLM API key is empty".to_string());
    }

    vf_cloud::list_models(&api_key)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn list_input_devices() -> Result<Vec<String>, String> {
    vf_engine::list_input_devices().map_err(|e| e.to_string())
}

#[tauri::command]
fn dictionary_list(store: State<'_, Arc<SqliteStore>>) -> Result<Vec<DictEntry>, String> {
    store.dictionary_list().map_err(|e| e.to_string())
}

#[tauri::command]
fn dictionary_add(word: String, source: String, store: State<'_, Arc<SqliteStore>>) -> Result<DictEntry, String> {
    store.dictionary_add(&word, &source).map_err(|e| e.to_string())
}

#[tauri::command]
fn dictionary_update(id: i64, word: String, store: State<'_, Arc<SqliteStore>>) -> Result<(), String> {
    store.dictionary_update(id, &word).map_err(|e| e.to_string())
}

#[tauri::command]
fn dictionary_delete(id: i64, store: State<'_, Arc<SqliteStore>>) -> Result<(), String> {
    store.dictionary_delete(id).map_err(|e| e.to_string())
}

#[tauri::command]
fn dictionary_toggle_star(id: i64, store: State<'_, Arc<SqliteStore>>) -> Result<(), String> {
    store.dictionary_toggle_star(id).map_err(|e| e.to_string())
}

#[tauri::command]
fn history_list(limit: u32, offset: u32, store: State<'_, Arc<SqliteStore>>) -> Result<Vec<HistoryEntry>, String> {
    store.history_list(limit, offset).map_err(|e| e.to_string())
}

#[tauri::command]
fn insights_summary(store: State<'_, Arc<SqliteStore>>) -> Result<InsightsSummary, String> {
    store.insights_summary().map_err(|e| e.to_string())
}

#[tauri::command]
fn scratchpad_get(store: State<'_, Arc<SqliteStore>>) -> Result<String, String> {
    store.scratchpad_get().map_err(|e| e.to_string())
}

#[tauri::command]
fn scratchpad_set(content: String, store: State<'_, Arc<SqliteStore>>) -> Result<(), String> {
    store.scratchpad_set(&content).map_err(|e| e.to_string())
}

#[tauri::command]
fn reset_prompt(name: String) -> Result<String, String> {
    match name.as_str() {
        "light" => Ok(vf_core::PROMPT_LIGHT.to_string()),
        "medium" => Ok(vf_core::PROMPT_MEDIUM.to_string()),
        "high" => Ok(vf_core::PROMPT_HIGH.to_string()),
        "command" => Ok(vf_core::PROMPT_COMMAND.to_string()),
        _ => Err(format!("Unknown prompt name: {name}")),
    }
}

#[tauri::command]
fn set_autostart(enabled: bool) -> Result<(), String> {
    use winreg::enums::*;
    use winreg::RegKey;
    
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = r"Software\Microsoft\Windows\CurrentVersion\Run";
    
    if enabled {
        let current_exe = std::env::current_exe()
            .map_err(|e| format!("Failed to get current exe path: {e}"))?;
        let current_exe_str = current_exe.to_string_lossy();
        
        let (key, _) = hkcu.create_subkey(path)
            .map_err(|e| format!("Failed to open registry key: {e}"))?;
        let val = current_exe_str.as_ref();
        key.set_value("VillFlow", &val)
            .map_err(|e| format!("Failed to write registry value: {e}"))?;
    } else {
        let key = hkcu.open_subkey_with_flags(path, KEY_WRITE)
            .map_err(|e| format!("Failed to open registry key for writing: {e}"))?;
        let _ = key.delete_value("VillFlow");
    }
    Ok(())
}

#[tauri::command]
fn autostart_status() -> Result<bool, String> {
    use winreg::enums::*;
    use winreg::RegKey;
    
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = r"Software\Microsoft\Windows\CurrentVersion\Run";
    
    let key = match hkcu.open_subkey_with_flags(path, KEY_READ) {
        Ok(k) => k,
        Err(_) => return Ok(false),
    };
    
    let value: Result<String, _> = key.get_value("VillFlow");
    Ok(value.is_ok())
}

fn main() {
    env_logger::init();

    let db_path = vf_store::get_default_db_path().expect("Could not resolve default DB path");
    let store = vf_store::SqliteStore::new(&db_path).expect("Failed to initialize SqliteStore");
    let store_state = Arc::new(store);

    let settings_path = vf_store::get_default_settings_path().expect("Could not resolve default Settings path");
    let settings = vf_store::load_settings(&settings_path).expect("Failed to load settings");

    // Spawn engine
    let engine_handle = vf_engine::spawn(settings.clone(), store_state.clone() as Arc<dyn Store>);
    let engine_handle_for_setup = engine_handle.subscribe();
    let engine_handle_for_state = engine_handle;

    let app_settings = AppSettings(std::sync::Mutex::new(settings));

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(store_state)
        .manage(engine_handle_for_state)
        .manage(app_settings)
        .setup(move |app| {
            let app_handle = app.handle().clone();

            // 1. Build Tray Menu
            let open_main = MenuItem::with_id(app, "open_main", "Open VillFlow", true, None::<&str>)?;
            let toggle_scratch = MenuItem::with_id(app, "toggle_scratch", "Scratchpad", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&open_main, &toggle_scratch, &quit])?;

            // 2. Build Tray Icon using the compiled PNG asset
            let tray_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))
                .expect("Failed to load 32x32.png icon");

            let _tray = TrayIconBuilder::with_id("main_tray")
                .icon(tray_icon)
                .menu(&tray_menu)
                .tooltip("VillFlow - Idle")
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, .. } = event {
                        let app = tray.app_handle();
                        if let Some(main_window) = app.get_webview_window("main") {
                            let _ = main_window.show();
                            let _ = main_window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // 3. Register Tray Menu click handler
            app.on_menu_event(move |app, event| {
                match event.id.as_ref() {
                    "open_main" => {
                        if let Some(main_window) = app.get_webview_window("main") {
                            let _ = main_window.show();
                            let _ = main_window.set_focus();
                        }
                    }
                    "toggle_scratch" => {
                        if let Some(scratchpad_window) = app.get_webview_window("scratchpad") {
                            if let Ok(visible) = scratchpad_window.is_visible() {
                                if visible {
                                    let _ = scratchpad_window.hide();
                                } else {
                                    let _ = scratchpad_window.show();
                                    let _ = scratchpad_window.set_focus();
                                }
                            }
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                }
            });

            // 4. Handle start_minimized configuration
            let main_window = app.get_webview_window("main").unwrap();
            let app_settings_state = app.state::<AppSettings>();
            let start_min = {
                let s = app_settings_state.0.lock().unwrap();
                s.general.start_minimized
            };
            if start_min {
                let _ = main_window.hide();
            } else {
                let _ = main_window.show();
            }

            // 5. Asynchronously bridge engine events
            let mut rx = engine_handle_for_setup;
            tauri::async_runtime::spawn(async move {
                while let Ok(event) = rx.recv().await {
                    match event {
                        EngineEvent::State(state) => {
                            let _ = app_handle.emit("engine-state", state);
                            if let Some(tray) = app_handle.tray_by_id("main_tray") {
                                let tooltip = match state {
                                    vf_core::EngineState::Idle => "VillFlow - Idle",
                                    vf_core::EngineState::Recording => "VillFlow - Recording",
                                    vf_core::EngineState::Processing => "VillFlow - Processing",
                                    vf_core::EngineState::Injecting => "VillFlow - Injecting",
                                };
                                let _ = tray.set_tooltip(Some(tooltip));
                            }
                        }
                        EngineEvent::Error(err_msg) => {
                            let _ = app_handle.emit("engine-error", err_msg.clone());
                            if let Some(tray) = app_handle.tray_by_id("main_tray") {
                                let _ = tray.set_tooltip(Some(&format!("VillFlow Error: {err_msg}")));
                            }

                            // Desktop Notification
                            let show_notif = {
                                let s = app_handle.state::<AppSettings>();
                                let val = s.0.lock().unwrap().general.show_error_notifications;
                                val
                            };
                            if show_notif {
                                let _ = app_handle.notification()
                                    .builder()
                                    .title("VillFlow Error")
                                    .body(&err_msg)
                                    .show();
                            }
                        }
                        EngineEvent::ToggleScratchpad => {
                            let _ = app_handle.emit("scratchpad-toggle", ());
                            if let Some(scratchpad_window) = app_handle.get_webview_window("scratchpad") {
                                if let Ok(visible) = scratchpad_window.is_visible() {
                                    if visible {
                                        let _ = scratchpad_window.hide();
                                    } else {
                                        let _ = scratchpad_window.show();
                                        let _ = scratchpad_window.set_focus();
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .on_menu_event(|_, _| {}) // satisfies callback requirement if needed
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            list_groq_models,
            list_input_devices,
            dictionary_list,
            dictionary_add,
            dictionary_update,
            dictionary_delete,
            dictionary_toggle_star,
            history_list,
            insights_summary,
            scratchpad_get,
            scratchpad_set,
            reset_prompt,
            set_autostart,
            autostart_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
