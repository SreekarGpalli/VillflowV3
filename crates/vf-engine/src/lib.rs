//! VillFlow engine — global hotkeys, audio, UIA context, injection, overlay, orchestrator.
//!
//! Owned by GrokBuild (CONTRACTS §4). Entry point: [`spawn`].

mod audio;
mod autolearn;
mod context;
mod hotkeys;
mod inject;
mod orchestrator;
mod overlay;
mod util;

use std::sync::Arc;
use std::thread;

use tokio::sync::{broadcast, mpsc};
use vf_core::{EngineCmd, EngineEvent, Settings, Store};

pub use audio::list_input_devices;
pub use hotkeys::{HotkeyEvent, HotkeyId, KeyCombo};

/// Handle to a running engine (command channel + event subscription factory).
pub struct EngineHandle {
    cmd_tx: mpsc::UnboundedSender<EngineCmd>,
    event_tx: broadcast::Sender<EngineEvent>,
}

impl EngineHandle {
    /// Send a command to the engine (e.g. [`EngineCmd::ApplySettings`], [`EngineCmd::Shutdown`]).
    pub fn send(&self, cmd: EngineCmd) -> Result<(), EngineSendError> {
        self.cmd_tx
            .send(cmd)
            .map_err(|_| EngineSendError::Closed)
    }

    /// Subscribe to engine events (state changes, errors, injected, etc.).
    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.event_tx.subscribe()
    }

    /// Direct access to the command sender (CONTRACTS §12 `cmd_tx`).
    pub fn cmd_tx(&self) -> mpsc::UnboundedSender<EngineCmd> {
        self.cmd_tx.clone()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EngineSendError {
    #[error("engine command channel closed")]
    Closed,
}

/// Spawn the engine on a dedicated OS thread owning a multi-thread tokio runtime.
///
/// Returns immediately with an [`EngineHandle`].
pub fn spawn(settings: Settings, store: Arc<dyn Store>) -> EngineHandle {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<EngineCmd>();
    let (event_tx, _) = broadcast::channel::<EngineEvent>(64);

    let event_tx_thread = event_tx.clone();
    thread::Builder::new()
        .name("vf-engine".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("vf-engine-worker")
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    log::error!("failed to build engine tokio runtime: {e}");
                    return;
                }
            };
            rt.block_on(orchestrator::run(
                settings,
                store,
                event_tx_thread,
                cmd_rx,
            ));
        })
        .expect("failed to spawn vf-engine thread");

    EngineHandle { cmd_tx, event_tx }
}
