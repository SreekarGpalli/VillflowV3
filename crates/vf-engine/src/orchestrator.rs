//! Engine orchestrator state machine — CONTRACTS §5.

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{broadcast, mpsc, Mutex};
use vf_cloud::{
    build_command, build_dictation, build_keyterms, chat_completion, PromptContext, SttSession,
};
use vf_core::{
    CleanupLevel, EngineCmd, EngineEvent, EngineState, HistoryEntry, Settings, Store,
};

use crate::audio::{self, AudioChunk, CaptureHandle};
use crate::autolearn;
use crate::context::{self, FocusContext};
use crate::hotkeys::{self, HotkeyEvent, HotkeyId};
use crate::inject;
use crate::overlay::{self, OverlayCmd};
use crate::util::{local_iso8601, word_count};

struct EngineRuntime {
    settings: Settings,
    store: Arc<dyn Store>,
    event_tx: broadcast::Sender<EngineEvent>,
    overlay_tx: mpsc::UnboundedSender<OverlayCmd>,
    state: EngineState,
    active: Option<ActiveUtterance>,
}

struct ActiveUtterance {
    mode: UtteranceMode,
    started: Instant,
    context: FocusContext,
    /// Shared so the feed task can call `feed_pcm` while we later `take` for `commit`.
    stt: Arc<Mutex<Option<SttSession>>>,
    capture: Option<CaptureHandle>,
    feed_task: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UtteranceMode {
    Dictation,
    Command,
}

impl EngineRuntime {
    fn emit(&self, ev: EngineEvent) {
        let _ = self.event_tx.send(ev);
    }

    fn set_state(&mut self, s: EngineState) {
        self.state = s;
        self.emit(EngineEvent::State(s));
    }

    fn abort_active(&mut self) {
        if let Some(mut active) = self.active.take() {
            if let Some(c) = active.capture.take() {
                c.stop();
            }
            if let Some(t) = active.feed_task.take() {
                t.abort();
            }
        }
    }

    fn error(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        log::error!("{msg}");
        self.emit(EngineEvent::Error(msg));
        self.abort_active();
        overlay::hide(&self.overlay_tx);
        self.set_state(EngineState::Idle);
    }
}

/// Main engine loop (runs on the engine thread's tokio runtime).
pub async fn run(
    settings: Settings,
    store: Arc<dyn Store>,
    event_tx: broadcast::Sender<EngineEvent>,
    mut cmd_rx: mpsc::UnboundedReceiver<EngineCmd>,
) {
    let overlay_tx = overlay::start();

    let mut hotkey_rx = match hotkeys::start(
        &settings.hotkeys.dictation,
        &settings.hotkeys.command_mode,
        &settings.hotkeys.scratchpad,
    ) {
        Ok(rx) => rx,
        Err(e) => {
            let _ = event_tx.send(EngineEvent::Error(format!("hotkey hook failed: {e}")));
            let (_tx, rx) = mpsc::unbounded_channel();
            rx
        }
    };

    let mut rt = EngineRuntime {
        settings,
        store,
        event_tx,
        overlay_tx,
        state: EngineState::Idle,
        active: None,
    };
    rt.emit(EngineEvent::State(EngineState::Idle));

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                match cmd {
                    None => break,
                    Some(EngineCmd::Shutdown) => {
                        rt.abort_active();
                        let _ = rt.overlay_tx.send(OverlayCmd::Shutdown);
                        break;
                    }
                    Some(EngineCmd::ApplySettings(s)) => {
                        rt.settings = *s;
                        if let Err(e) = hotkeys::update_combos(
                            &rt.settings.hotkeys.dictation,
                            &rt.settings.hotkeys.command_mode,
                            &rt.settings.hotkeys.scratchpad,
                        ) {
                            rt.emit(EngineEvent::Error(format!("hotkey update failed: {e}")));
                        }
                    }
                }
            }

            ev = hotkey_rx.recv() => {
                match ev {
                    None => break,
                    Some(HotkeyEvent::Down(HotkeyId::Scratchpad)) => {
                        rt.emit(EngineEvent::ToggleScratchpad);
                    }
                    Some(HotkeyEvent::Up(HotkeyId::Scratchpad)) => {}
                    Some(HotkeyEvent::Down(HotkeyId::Dictation)) => {
                        if rt.state == EngineState::Idle {
                            begin_utterance(&mut rt, UtteranceMode::Dictation).await;
                        }
                    }
                    Some(HotkeyEvent::Up(HotkeyId::Dictation)) => {
                        if matches!(
                            rt.active.as_ref().map(|a| a.mode),
                            Some(UtteranceMode::Dictation)
                        ) {
                            finish_utterance(&mut rt).await;
                        }
                    }
                    Some(HotkeyEvent::Down(HotkeyId::CommandMode)) => {
                        if rt.state == EngineState::Idle {
                            let selection = tokio::task::spawn_blocking(
                                context::read_selection_with_fallback,
                            )
                            .await
                            .ok()
                            .flatten();
                            match selection {
                                None => {
                                    overlay::show_toast(&rt.overlay_tx, "Select text first");
                                }
                                Some(sel) => {
                                    begin_utterance(&mut rt, UtteranceMode::Command).await;
                                    if let Some(active) = rt.active.as_mut() {
                                        active.context.selection = Some(sel);
                                    }
                                }
                            }
                        }
                    }
                    Some(HotkeyEvent::Up(HotkeyId::CommandMode)) => {
                        if matches!(
                            rt.active.as_ref().map(|a| a.mode),
                            Some(UtteranceMode::Command)
                        ) {
                            finish_utterance(&mut rt).await;
                        }
                    }
                }
            }
        }
    }
}

async fn begin_utterance(rt: &mut EngineRuntime, mode: UtteranceMode) {
    rt.set_state(EngineState::Recording);
    overlay::show_recording(&rt.overlay_tx, 0.0);

    let context = tokio::task::spawn_blocking(context::read_focus_context)
        .await
        .unwrap_or_default();

    let keyterms = match rt.store.dictionary_list() {
        Ok(entries) => build_keyterms(&entries),
        Err(e) => {
            log::warn!("dictionary_list failed: {e}");
            Vec::new()
        }
    };

    let session = match SttSession::open(rt.settings.stt.clone(), keyterms).await {
        Ok(s) => s,
        Err(e) => {
            rt.error(format!("STT open failed: {e}"));
            return;
        }
    };

    let stt = Arc::new(Mutex::new(Some(session)));

    let (audio_tx, mut audio_rx) = mpsc::unbounded_channel::<AudioChunk>();
    let capture = match audio::start_capture(&rt.settings.audio.input_device, audio_tx) {
        Ok(c) => Some(c),
        Err(e) => {
            rt.error(format!("audio capture failed: {e}"));
            return;
        }
    };

    let stt_for_feed = stt.clone();
    let overlay_tx = rt.overlay_tx.clone();
    let feed_task = tokio::spawn(async move {
        while let Some(chunk) = audio_rx.recv().await {
            overlay::show_recording(&overlay_tx, chunk.rms);
            let guard = stt_for_feed.lock().await;
            if let Some(session) = guard.as_ref() {
                if let Err(e) = session.feed_pcm(&chunk.pcm_s16le).await {
                    log::warn!("STT feed error: {e}");
                }
            } else {
                break;
            }
        }
    });

    rt.active = Some(ActiveUtterance {
        mode,
        started: Instant::now(),
        context,
        stt,
        capture,
        feed_task: Some(feed_task),
    });
}

async fn finish_utterance(rt: &mut EngineRuntime) {
    let Some(mut active) = rt.active.take() else {
        return;
    };

    // Stop capture first so the feed task drains and exits.
    if let Some(c) = active.capture.take() {
        c.stop();
    }
    if let Some(t) = active.feed_task.take() {
        // Give the feed task a moment to flush last chunks, then abort if needed.
        let _ = tokio::time::timeout(std::time::Duration::from_millis(200), t).await;
    }

    rt.set_state(EngineState::Processing);
    overlay::show_processing(&rt.overlay_tx);

    let session = {
        let mut guard = active.stt.lock().await;
        guard.take()
    };
    let Some(session) = session else {
        rt.error("STT session missing");
        return;
    };

    let raw_transcript = match session.commit().await {
        Ok(t) => t,
        Err(e) => {
            rt.error(format!("STT commit failed: {e}"));
            return;
        }
    };

    let mode = active.mode;
    let ctx = active.context;
    let started = active.started;

    let final_text = match mode {
        UtteranceMode::Dictation => {
            match rt.settings.llm.cleanup_level {
                CleanupLevel::None => raw_transcript.clone(),
                level => {
                    let dict_words = dictionary_words_ordered(&rt.store);
                    let prompt_ctx = PromptContext {
                        app_name: ctx.app_name.clone(),
                        field_context: ctx.field_context.clone(),
                        dictionary: dict_words,
                    };
                    match build_dictation(
                        level,
                        &rt.settings.prompts,
                        &prompt_ctx,
                        &raw_transcript,
                    ) {
                        None => raw_transcript.clone(),
                        Some(msgs) => {
                            match chat_completion(
                                &msgs.system,
                                &msgs.user,
                                &rt.settings.llm.model,
                                &rt.settings.llm.api_key,
                            )
                            .await
                            {
                                Ok(text) => text,
                                Err(e) => {
                                    rt.error(format!("Groq failed: {e}"));
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        }
        UtteranceMode::Command => {
            let selection = ctx.selection.clone().unwrap_or_default();
            let dict_words = dictionary_words_ordered(&rt.store);
            let prompt_ctx = PromptContext {
                app_name: ctx.app_name.clone(),
                field_context: ctx.field_context.clone(),
                dictionary: dict_words,
            };
            let msgs = build_command(
                &rt.settings.prompts,
                &prompt_ctx,
                &raw_transcript, // spoken instruction
                &selection,
            );
            match chat_completion(
                &msgs.system,
                &msgs.user,
                &rt.settings.llm.model,
                &rt.settings.llm.api_key,
            )
            .await
            {
                Ok(text) => text,
                Err(e) => {
                    rt.error(format!("Groq failed: {e}"));
                    return;
                }
            }
        }
    };

    rt.set_state(EngineState::Injecting);

    let method = rt.settings.output.injection_method;
    let restore = rt.settings.output.restore_clipboard;
    let inject_text = final_text.clone();
    let inject_result =
        tokio::task::spawn_blocking(move || inject::inject_text(&inject_text, method, restore))
            .await;

    match inject_result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            rt.error(format!("injection failed: {e}"));
            return;
        }
        Err(e) => {
            rt.error(format!("injection task failed: {e}"));
            return;
        }
    }

    let total_ms = started.elapsed().as_millis() as u64;
    let words = word_count(&final_text);

    let history_mode = match mode {
        UtteranceMode::Dictation => "dictation",
        UtteranceMode::Command => "command",
    };
    let entry = HistoryEntry {
        id: None,
        ts: local_iso8601(),
        app_name: ctx.app_name.clone(),
        window_title: ctx.window_title.clone(),
        mode: history_mode.to_string(),
        raw_transcript: raw_transcript.clone(),
        final_text: final_text.clone(),
        duration_ms: total_ms as i64,
        word_count: words as i64,
    };
    if let Err(e) = rt.store.history_append(&entry) {
        log::warn!("history_append failed: {e}");
    }

    rt.emit(EngineEvent::Injected {
        words,
        total_ms,
    });

    // Auto-learn + use_count bump (§15).
    autolearn::spawn_auto_learn(
        rt.store.clone(),
        final_text,
        rt.settings.dictionary.auto_learn,
        rt.event_tx.clone(),
    );

    overlay::hide(&rt.overlay_tx);
    rt.set_state(EngineState::Idle);
}

fn dictionary_words_ordered(store: &Arc<dyn Store>) -> Vec<String> {
    match store.dictionary_list() {
        Ok(entries) => {
            // Starred first, then use_count — same ordering spirit as keyterms.
            let mut entries = entries;
            entries.sort_by(|a, b| {
                b.starred
                    .cmp(&a.starred)
                    .then_with(|| b.use_count.cmp(&a.use_count))
                    .then_with(|| a.word.cmp(&b.word))
            });
            entries.into_iter().map(|e| e.word).collect()
        }
        Err(_) => Vec::new(),
    }
}
