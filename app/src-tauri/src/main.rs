// Prevents additional console window on Windows in release, do not remove.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use tauri::State;
use vf_core::{DictEntry, HistoryEntry, InsightsSummary, Settings, Store};
use vf_store::SqliteStore;

#[tauri::command]
fn get_settings() -> Result<Settings, String> {
    let settings_path = vf_store::get_default_settings_path()
        .ok_or_else(|| "Could not resolve settings path".to_string())?;
    vf_store::load_settings(&settings_path)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn save_settings(settings: Settings) -> Result<(), String> {
    let settings_path = vf_store::get_default_settings_path()
        .ok_or_else(|| "Could not resolve settings path".to_string())?;
    vf_store::save_settings(&settings, &settings_path)
        .map_err(|e| e.to_string())?;
    
    // TODO(vf): wired in P5 - EngineCmd::ApplySettings
    
    Ok(())
}

#[tauri::command]
async fn list_groq_models() -> Result<Vec<String>, String> {
    let settings_path = vf_store::get_default_settings_path()
        .ok_or_else(|| "Could not resolve settings path".to_string())?;
    let settings = vf_store::load_settings(&settings_path)
        .map_err(|e| e.to_string())?;
    
    if settings.llm.api_key.trim().is_empty() {
        return Err("LLM API key is empty".to_string());
    }

    vf_cloud::list_models(&settings.llm.api_key)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn list_input_devices() -> Result<Vec<String>, String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let devices = host.input_devices()
        .map_err(|e| format!("Failed to get input devices: {e}"))?;
    
    let mut names = vec!["system_default".to_string()];
    for device in devices {
        if let Ok(name) = device.name() {
            names.push(name);
        }
    }
    Ok(names)
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
    let db_path = vf_store::get_default_db_path().expect("Could not resolve default DB path");
    let store = vf_store::SqliteStore::new(&db_path).expect("Failed to initialize SqliteStore");
    let store_state = Arc::new(store);

    tauri::Builder::default()
        .manage(store_state)
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
