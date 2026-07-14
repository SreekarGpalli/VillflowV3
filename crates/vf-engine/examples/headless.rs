//! Headless engine runner for manual testing without the Tauri shell.
//!
//! Usage:
//!   cargo run -p vf-engine --example headless
//!   cargo run -p vf-engine --example headless -- path\to\settings.json
//!
//! Loads settings + SQLite store from `%APPDATA%\VillFlow\` (or the given settings path),
//! starts the engine, and prints `EngineEvent`s to stdout until Ctrl+C.

use std::path::PathBuf;
use std::sync::Arc;

use vf_core::{EngineCmd, EngineEvent};
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
    println!("Press Ctrl+C to shut down cleanly.");
    println!("Events:");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    rt.block_on(async move {
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    println!("Ctrl+C — shutting down…");
                    let _ = handle.send(EngineCmd::Shutdown);
                    // Give the engine a moment to unhook / hide overlay.
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    break;
                }
                ev = events.recv() => {
                    match ev {
                        Ok(ev) => print_event(&ev),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            eprintln!("(lagged {n} events)");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            eprintln!("event channel closed");
                            break;
                        }
                    }
                }
            }
        }
    });
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
        EngineEvent::AppInsert { text } => {
            println!("[app-insert] {} chars (in-process UI delivery)", text.chars().count());
        }
    }
}
