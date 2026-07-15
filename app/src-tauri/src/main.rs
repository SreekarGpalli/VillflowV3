// Prevents additional console window on Windows in release, do not remove.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_notification::NotificationExt;
use vf_core::{
    DictEntry, EngineEvent, HistoryEntry, InsightsSummary, Settings, Store,
};
use vf_engine::EngineHandle;
use vf_store::SqliteStore;

pub struct AppSettings(pub std::sync::Mutex<Settings>);

/// Tracks whether the Scratchpad should be considered open.
/// We intentionally do NOT trust `is_visible()` alone — it lies for
/// always-on-top / hide-to-tray windows on Windows.
pub struct ScratchpadUi {
    open: AtomicBool,
    last_toggle: std::sync::Mutex<Option<Instant>>,
}

impl Default for ScratchpadUi {
    fn default() -> Self {
        Self {
            open: AtomicBool::new(false),
            last_toggle: std::sync::Mutex::new(None),
        }
    }
}

/// Show or hide the Scratchpad. Shared by hotkey + tray.
fn toggle_scratchpad_window(app: &AppHandle) {
    let Some(win) = app.get_webview_window("scratchpad") else {
        log::error!("scratchpad window missing");
        return;
    };
    let ui = app.state::<ScratchpadUi>();

    // Short debounce only (key-repeat protection). 120ms is enough; 350ms felt
    // like a "broken" hotkey when users double-tapped quickly.
    {
        let mut last = ui.last_toggle.lock().unwrap();
        let now = Instant::now();
        if let Some(t) = *last {
            if now.duration_since(t) < Duration::from_millis(120) {
                return;
            }
        }
        *last = Some(now);
    }

    // Pure flip of our flag — ignore flaky is_visible for the decision.
    let currently_open = ui.open.load(Ordering::SeqCst);

    if currently_open {
        let _ = win.hide();
        ui.open.store(false, Ordering::SeqCst);
        log::info!("scratchpad: hide");
    } else {
        let _ = win.unminimize();
        if let Err(e) = win.show() {
            log::error!("scratchpad show failed: {e}");
        }
        let _ = win.set_always_on_top(true);
        let _ = win.set_focus();
        ui.open.store(true, Ordering::SeqCst);
        // Focus the contenteditable after the window is up.
        let _ = win.emit("scratchpad-focus", ());
        log::info!("scratchpad: show");
    }
}

/// Deliver dictation text into our WebViews (Scratchpad / main).
///
/// Only the focused VillFlow window receives the insert. Emitting to both
/// windows previously polluted the Scratchpad whenever the user dictated into
/// a Settings field (and vice-versa).
fn emit_app_insert(app: &AppHandle, text: &str) {
    let labels = ["scratchpad", "main"];
    for label in labels {
        if let Some(w) = app.get_webview_window(label) {
            if w.is_focused().unwrap_or(false) {
                let _ = w.emit("app-insert", text);
                return;
            }
        }
    }
    // Neither reports focus (common with WebView2 focus quirks): prefer an
    // open Scratchpad, else the main window.
    if let Some(ui) = app.try_state::<ScratchpadUi>() {
        if ui.open.load(Ordering::SeqCst) {
            if let Some(w) = app.get_webview_window("scratchpad") {
                let _ = w.emit("app-insert", text);
                return;
            }
        }
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.emit("app-insert", text);
    }
}

#[tauri::command]
fn get_settings(settings_state: State<'_, AppSettings>) -> Result<Settings, String> {
    Ok(settings_state.0.lock().unwrap().clone())
}

/// Validate hotkey combos before persisting. Requires at least one modifier
/// per combo (never swallow bare letter keys system-wide) and unique combos.
fn validate_hotkeys(settings: &Settings) -> Result<(), String> {
    use vf_engine::KeyCombo;

    let dictation = KeyCombo::parse(&settings.hotkeys.dictation)
        .ok_or_else(|| format!("Invalid dictation hotkey: {}", settings.hotkeys.dictation))?;
    let command = KeyCombo::parse(&settings.hotkeys.command_mode).ok_or_else(|| {
        format!(
            "Invalid command mode hotkey: {}",
            settings.hotkeys.command_mode
        )
    })?;
    let scratchpad = KeyCombo::parse(&settings.hotkeys.scratchpad)
        .ok_or_else(|| format!("Invalid scratchpad hotkey: {}", settings.hotkeys.scratchpad))?;

    for (name, combo) in [
        ("dictation", &dictation),
        ("command mode", &command),
        ("scratchpad", &scratchpad),
    ] {
        if !(combo.ctrl || combo.shift || combo.alt || combo.win) {
            return Err(format!(
                "Hotkey for {name} must include at least one modifier (Ctrl/Shift/Alt/Win)"
            ));
        }
    }

    if dictation == command {
        return Err("Dictation and command mode hotkeys must be different".into());
    }
    if dictation == scratchpad {
        return Err("Dictation and scratchpad hotkeys must be different".into());
    }
    if command == scratchpad {
        return Err("Command mode and scratchpad hotkeys must be different".into());
    }

    Ok(())
}

#[tauri::command]
fn save_settings(
    settings: Settings,
    settings_state: State<'_, AppSettings>,
    _store: State<'_, Arc<SqliteStore>>,
    engine: State<'_, EngineHandle>,
) -> Result<(), String> {
    validate_hotkeys(&settings)?;

    let settings_path = vf_store::get_default_settings_path()
        .ok_or_else(|| "Could not resolve settings path".to_string())?;
    vf_store::save_settings(&settings, &settings_path).map_err(|e| e.to_string())?;

    *settings_state.0.lock().unwrap() = settings.clone();

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
fn sample_mic_level(device: String) -> Result<f32, String> {
    let dev = if device.trim().is_empty() {
        "system_default".to_string()
    } else {
        device
    };
    vf_engine::sample_mic_level(&dev, 450).map_err(|e| e.to_string())
}

#[tauri::command]
fn history_export(store: State<'_, Arc<SqliteStore>>) -> Result<String, String> {
    let rows = store.history_list(10_000, 0).map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())
}

#[tauri::command]
fn dictionary_export(store: State<'_, Arc<SqliteStore>>) -> Result<String, String> {
    let rows = store.dictionary_list().map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())
}

#[tauri::command]
fn dictionary_list(store: State<'_, Arc<SqliteStore>>) -> Result<Vec<DictEntry>, String> {
    store.dictionary_list().map_err(|e| e.to_string())
}

#[tauri::command]
fn dictionary_add(
    word: String,
    source: String,
    store: State<'_, Arc<SqliteStore>>,
) -> Result<DictEntry, String> {
    store.dictionary_add(&word, &source).map_err(|e| e.to_string())
}

#[tauri::command]
fn dictionary_update(
    id: i64,
    word: String,
    store: State<'_, Arc<SqliteStore>>,
) -> Result<(), String> {
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
fn history_list(
    limit: u32,
    offset: u32,
    store: State<'_, Arc<SqliteStore>>,
) -> Result<Vec<HistoryEntry>, String> {
    store.history_list(limit, offset).map_err(|e| e.to_string())
}

#[tauri::command]
fn history_delete(id: i64, store: State<'_, Arc<SqliteStore>>) -> Result<(), String> {
    store.history_delete(id).map_err(|e| e.to_string())
}

#[tauri::command]
fn history_clear(store: State<'_, Arc<SqliteStore>>) -> Result<(), String> {
    store.history_clear().map_err(|e| e.to_string())
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
        "command_generate" => Ok(vf_core::PROMPT_COMMAND_GENERATE.to_string()),
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
        let val = format!("\"{}\"", current_exe.display());

        let (key, _) = hkcu
            .create_subkey(path)
            .map_err(|e| format!("Failed to open registry key: {e}"))?;
        key.set_value("VillFlow", &val)
            .map_err(|e| format!("Failed to write registry value: {e}"))?;
    } else {
        let key = hkcu
            .open_subkey_with_flags(path, KEY_WRITE)
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

/// Initialize logging to stderr + `%APPDATA%\VillFlow\logs\villflow.log` (§3).
fn init_logging() {
    use std::fs::OpenOptions;

    let env = env_logger::Env::default().default_filter_or("info");
    let mut builder = env_logger::Builder::from_env(env);

    if let Some(log_path) = dirs::config_dir().map(|p| p.join("VillFlow").join("logs").join("villflow.log"))
    {
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match OpenOptions::new().create(true).append(true).open(&log_path) {
            Ok(file) => {
                let file = std::sync::Mutex::new(file);
                builder.target(env_logger::Target::Pipe(Box::new(WriteAdapter(file))));
                // Also keep a stderr fallback for `tauri dev` console visibility.
                // env_logger only supports one target; dual-write via adapter.
                let _ = writeln!(
                    std::io::stderr(),
                    "VillFlow logging to {}",
                    log_path.display()
                );
            }
            Err(e) => {
                let _ = writeln!(
                    std::io::stderr(),
                    "VillFlow: could not open log file ({}): {e}",
                    log_path.display()
                );
            }
        }
    }

    let _ = builder.try_init();
}

/// Mutex-guarded file that implements `Write` for env_logger.
struct WriteAdapter(std::sync::Mutex<std::fs::File>);

impl Write for WriteAdapter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut file = self
            .0
            .lock()
            .map_err(|_| std::io::Error::other("log file mutex poisoned"))?;
        // Mirror to stderr so `tauri dev` still shows logs.
        let _ = std::io::stderr().write_all(buf);
        file.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut file = self
            .0
            .lock()
            .map_err(|_| std::io::Error::other("log file mutex poisoned"))?;
        let _ = std::io::stderr().flush();
        file.flush()
    }
}

fn main() {
    init_logging();

    let db_path = match vf_store::get_default_db_path() {
        Some(p) => p,
        None => {
            log::error!("Could not resolve %APPDATA%\\VillFlow path");
            eprintln!("VillFlow: could not resolve application data directory.");
            std::process::exit(1);
        }
    };
    let store = match vf_store::SqliteStore::new(&db_path) {
        Ok(s) => s,
        Err(e) => {
            log::error!("Failed to initialize database: {e}");
            eprintln!("VillFlow: failed to open database: {e}");
            std::process::exit(1);
        }
    };
    let store_state = Arc::new(store);

    let settings_path = match vf_store::get_default_settings_path() {
        Some(p) => p,
        None => {
            log::error!("Could not resolve settings path");
            eprintln!("VillFlow: could not resolve settings path.");
            std::process::exit(1);
        }
    };
    let settings = match vf_store::load_settings(&settings_path) {
        Ok(s) => s,
        Err(e) => {
            log::error!("Failed to load settings: {e}");
            eprintln!("VillFlow: failed to load settings: {e}");
            std::process::exit(1);
        }
    };

    // History retention purge at startup (E2).
    let retention_days = settings.general.history_retention_days;
    if retention_days > 0 {
        match store_state.history_purge_older_than_days(retention_days) {
            Ok(n) if n > 0 => log::info!("purged {n} history row(s) older than {retention_days} days"),
            Ok(_) => {}
            Err(e) => log::warn!("history purge on startup failed: {e}"),
        }
    }

    let engine_handle = vf_engine::spawn(settings.clone(), store_state.clone() as Arc<dyn Store>);
    let engine_handle_for_setup = engine_handle.subscribe();
    let engine_handle_for_state = engine_handle;

    let app_settings = AppSettings(std::sync::Mutex::new(settings));

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(store_state)
        .manage(engine_handle_for_state)
        .manage(app_settings)
        .manage(ScratchpadUi::default())
        .setup(move |app| {
            let app_handle = app.handle().clone();

            let open_main =
                MenuItem::with_id(app, "open_main", "Open VillFlow", true, None::<&str>)?;
            let toggle_scratch =
                MenuItem::with_id(app, "toggle_scratch", "Scratchpad", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&open_main, &toggle_scratch, &quit])?;

            let tray_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))
                .expect("Failed to load 32x32.png icon");

            let _tray = TrayIconBuilder::with_id("main_tray")
                .icon(tray_icon)
                .menu(&tray_menu)
                .tooltip("VillFlow - Idle")
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(main_window) = app.get_webview_window("main") {
                            let _ = main_window.show();
                            let _ = main_window.set_focus();
                        }
                    }
                })
                .build(app)?;

            app.on_menu_event(move |app, event| match event.id.as_ref() {
                "open_main" => {
                    if let Some(main_window) = app.get_webview_window("main") {
                        let _ = main_window.show();
                        let _ = main_window.set_focus();
                    }
                }
                "toggle_scratch" => {
                    toggle_scratchpad_window(app);
                }
                "quit" => {
                    if let Some(engine) = app.try_state::<EngineHandle>() {
                        let _ = engine.send(vf_core::EngineCmd::Shutdown);
                    }
                    std::thread::sleep(Duration::from_millis(150));
                    app.exit(0);
                }
                _ => {}
            });

            let main_window = app.get_webview_window("main").unwrap();
            let app_settings_state = app.state::<AppSettings>();
            // PRODUCT: always show main until Ready (keys + config). Only then honor start_minimized.
            let (start_min, is_ready) = {
                let s = app_settings_state.0.lock().unwrap();
                let has_el = s.stt.api_keys.iter().any(|k| !k.trim().is_empty());
                let has_groq = !s.llm.api_key.trim().is_empty()
                    || matches!(s.llm.cleanup_level, vf_core::CleanupLevel::None);
                (s.general.start_minimized, has_el && has_groq)
            };
            if start_min && is_ready {
                let _ = main_window.hide();
            } else {
                let _ = main_window.show();
                let _ = main_window.set_focus();
            }

            // Pre-warm Scratchpad webview so listeners are registered before first use.
            if let Some(sp) = app.get_webview_window("scratchpad") {
                // Show briefly off-logic: ensure page loads while still "closed".
                // Just accessing the window is enough if it was created at startup;
                // emit a no-op to force webview wake on some Windows builds.
                let _ = sp.eval("void 0");
            }

            let mut rx = engine_handle_for_setup;
            tauri::async_runtime::spawn(async move {
                while let Ok(event) = rx.recv().await {
                    match event {
                        EngineEvent::State(state) => {
                            // Emit a plain string so every frontend gets "Recording" etc.
                            let state_str = match state {
                                vf_core::EngineState::Idle => "Idle",
                                vf_core::EngineState::Recording => "Recording",
                                vf_core::EngineState::Processing => "Processing",
                                vf_core::EngineState::Injecting => "Injecting",
                            };
                            let _ = app_handle.emit("engine-state", state_str);
                            // Also target Scratchpad directly (always-on-top window).
                            if let Some(sp) = app_handle.get_webview_window("scratchpad") {
                                let _ = sp.emit("engine-state", state_str);
                            }
                            if let Some(tray) = app_handle.tray_by_id("main_tray") {
                                let tooltip = format!("VillFlow - {state_str}");
                                let _ = tray.set_tooltip(Some(tooltip.as_str()));
                            }
                        }
                        EngineEvent::Error(err_msg) => {
                            let _ = app_handle.emit("engine-error", err_msg.clone());
                            if let Some(tray) = app_handle.tray_by_id("main_tray") {
                                let _ =
                                    tray.set_tooltip(Some(&format!("VillFlow Error: {err_msg}")));
                            }

                            let show_notif = {
                                let s = app_handle.state::<AppSettings>();
                                let val = s.0.lock().unwrap().general.show_error_notifications;
                                val
                            };
                            if show_notif {
                                let _ = app_handle
                                    .notification()
                                    .builder()
                                    .title("VillFlow Error")
                                    .body(&err_msg)
                                    .show();
                            }
                        }
                        EngineEvent::ToggleScratchpad => {
                            toggle_scratchpad_window(&app_handle);
                        }
                        EngineEvent::AppInsert { text } => {
                            log::info!("app-insert: {} chars", text.chars().count());
                            emit_app_insert(&app_handle, &text);
                        }
                        EngineEvent::Injected { words, total_ms } => {
                            let _ = app_handle.emit(
                                "engine-injected",
                                serde_json::json!({
                                    "words": words,
                                    "total_ms": total_ms,
                                }),
                            );
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
                if window.label() == "scratchpad" {
                    if let Some(ui) = window.try_state::<ScratchpadUi>() {
                        ui.open.store(false, Ordering::SeqCst);
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            list_groq_models,
            list_input_devices,
            sample_mic_level,
            dictionary_list,
            dictionary_add,
            dictionary_update,
            dictionary_delete,
            dictionary_toggle_star,
            history_list,
            history_delete,
            history_clear,
            history_export,
            dictionary_export,
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
