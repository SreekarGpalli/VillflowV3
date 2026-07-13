//! Headless engine runner for manual testing without the Tauri shell.
//!
//! Usage:
//!   cargo run -p vf-engine --example headless
//!   cargo run -p vf-engine --example headless -- path\to\settings.json
//!
//! Loads settings + SQLite store from `%APPDATA%\VillFlow\` (or the given settings path),
//! starts the engine, and prints `EngineEvent`s to stdout until Ctrl+C / process kill.

use std::path::PathBuf;
use std::sync::Arc;

use vf_core::{EngineCmd, EngineEvent, EngineState};
use vf_engine::spawn;
use vf_store::{get_default_db_path, get_default_settings_path, load_settings, SqliteStore};

fn main() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();

    let settings_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(get_default_settings_path)
        .expect("no settings path (pass one or ensure %APPDATA% is available)");

    let db_path = get_default_db_path().expect("no db path");

    println!("VillFlow headless engine");
    println!("  settings: {}", settings_path.display());
    println!("  database: {}", db_path.display());

    let settings = load_settings(&settings_path).expect("load settings");
    let store = Arc::new(SqliteStore::new(&db_path).expect("open store"));

    let handle = spawn(settings, store);
    let mut events = handle.subscribe();

    println!("Engine started. Hold Ctrl+Shift+Z to dictate, Ctrl+Shift+X for command mode.");
    println!("Events:");

    // Block on events in a tiny tokio runtime (broadcast recv is async).
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    rt.block_on(async move {
        loop {
            match events.recv().await {
                Ok(ev) => {
                    print_event(&ev);
                    if matches!(ev, EngineEvent::State(EngineState::Idle)) {
                        // Keep running; Idle is normal.
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("(lagged {n} events)");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    eprintln!("event channel closed");
                    break;
                }
            }
        }
    });

    let _ = handle.send(EngineCmd::Shutdown);
}

fn print_event(ev: &EngineEvent) {
    match ev {
        EngineEvent::State(s) => println!("[state] {s:?}"),
        EngineEvent::Error(e) => println!("[error] {e}"),
        EngineEvent::Injected { words, total_ms } => {
            println!("[injected] words={words} total_ms={total_ms}");
        }
        EngineEvent::ToggleScratchpad => println!("[scratchpad] toggle"),
        EngineEvent::DictionaryLearned(w) => println!("[auto-learn] {w}"),
    }
}
