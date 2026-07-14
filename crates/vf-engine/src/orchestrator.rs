//! Engine orchestrator state machine — CONTRACTS §5.

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{broadcast, mpsc, Mutex};
use vf_cloud::{
    build_command, build_command_generate, build_dictation, build_keyterms, chat_completion,
    dictation_output_suspicious, strip_context_echo, PromptContext, SttSession,
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
use crate::util::{local_iso8601, tail_chars, word_count};

/// Field-context cap sent to the LLM: enough for continuity, not enough to
/// tempt the model into rewriting the document.
const FIELD_CONTEXT_MAX_CHARS: usize = 600;

struct EngineRuntime {
    settings: Settings,
    store: Arc<dyn Store>,
    event_tx: broadcast::Sender<EngineEvent>,
    overlay_tx: mpsc::UnboundedSender<OverlayCmd>,
    state: EngineState,
    active: Option<ActiveUtterance>,
    /// Set when key-up arrives while `begin_utterance` is still opening STT.
    /// Finished as soon as the active utterance is ready.
    pending_finish: bool,
    /// Mode being started (used with `pending_finish` before `active` exists).
    pending_mode: Option<UtteranceMode>,
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
    /// Transform selected text.
    CommandEdit,
    /// Generate new content (no selection).
    CommandGenerate,
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
        self.pending_finish = false;
        self.pending_mode = None;
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
        // Errors often fire while the user still has Ctrl/Shift half-held from PTT.
        inject::force_release_all_modifiers();
        overlay::hide(&self.overlay_tx);
        self.set_state(EngineState::Idle);
    }

    fn request_finish_if_matching(&mut self, mode: UtteranceMode) -> bool {
        if matches!(self.active.as_ref().map(|a| a.mode), Some(m) if m == mode) {
            return true; // caller should finish_utterance
        }
        // Key released while still opening STT/capture for this mode.
        if self.pending_mode == Some(mode) && self.active.is_none() {
            self.pending_finish = true;
        }
        false
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
        pending_finish: false,
        pending_mode: None,
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
                        hotkeys::shutdown();
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
                        // Toggle only when idle — avoid hiding Scratchpad mid-dictation.
                        // Edge-trigger is the main key down (not modifier releases).
                        if rt.state == EngineState::Idle {
                            rt.emit(EngineEvent::ToggleScratchpad);
                        }
                    }
                    Some(HotkeyEvent::Up(HotkeyId::Scratchpad)) => {
                        // No-op: scratchpad is a press-to-toggle, not push-to-talk.
                    }
                    Some(HotkeyEvent::Down(HotkeyId::Dictation)) => {
                        if rt.state == EngineState::Idle {
                            begin_utterance(&mut rt, UtteranceMode::Dictation).await;
                        }
                    }
                    Some(HotkeyEvent::Up(HotkeyId::Dictation)) => {
                        if rt.request_finish_if_matching(UtteranceMode::Dictation) {
                            finish_utterance(&mut rt).await;
                        }
                    }
                    Some(HotkeyEvent::Down(HotkeyId::CommandMode)) => {
                        if rt.state == EngineState::Idle {
                            // Selection is optional: with one, the spoken command
                            // transforms it; without, the command generates new
                            // content inserted at the cursor.
                            let selection = tokio::task::spawn_blocking(
                                context::read_selection_with_fallback,
                            )
                            .await
                            .ok()
                            .flatten()
                            .filter(|s| !s.trim().is_empty());
                            let mode = if selection.is_some() {
                                UtteranceMode::CommandEdit
                            } else {
                                UtteranceMode::CommandGenerate
                            };
                            begin_utterance(&mut rt, mode).await;
                            if let Some(active) = rt.active.as_mut() {
                                active.context.selection = selection;
                            }
                        }
                    }
                    Some(HotkeyEvent::Up(HotkeyId::CommandMode)) => {
                        // Finish either edit or generate command utterances.
                        let should = matches!(
                            rt.active.as_ref().map(|a| a.mode),
                            Some(UtteranceMode::CommandEdit | UtteranceMode::CommandGenerate)
                        );
                        if should {
                            finish_utterance(&mut rt).await;
                        } else if matches!(
                            rt.pending_mode,
                            Some(UtteranceMode::CommandEdit | UtteranceMode::CommandGenerate)
                        ) && rt.active.is_none()
                        {
                            rt.pending_finish = true;
                        }
                    }
                }
            }
        }
    }
}

async fn begin_utterance(rt: &mut EngineRuntime, mode: UtteranceMode) {
    // Clock from key-down (start of begin), not after STT open — §5 duration_ms.
    let started = Instant::now();
    rt.pending_mode = Some(mode);
    rt.pending_finish = false;
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
        started,
        context,
        stt,
        capture,
        feed_task: Some(feed_task),
    });
    rt.pending_mode = None;

    // Key-up arrived while STT/capture was still starting — finish immediately.
    if rt.pending_finish {
        rt.pending_finish = false;
        finish_utterance(rt).await;
    }
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

    // Empty / silence — do not inject or pollute history.
    // Toast auto-hides via overlay expiry; do NOT call hide() here (it cancels the toast).
    if raw_transcript.trim().is_empty() {
        inject::force_release_all_modifiers();
        overlay::show_toast(&rt.overlay_tx, "No speech detected");
        rt.set_state(EngineState::Idle);
        return;
    }

    let mode = active.mode;
    let mut ctx = active.context;
    let started = active.started;

    // For command-edit: refresh selection just before the LLM so we transform
    // what is currently selected if the user adjusted it mid-hold.
    if mode == UtteranceMode::CommandEdit {
        let fresh = tokio::task::spawn_blocking(context::read_selection_with_fallback)
            .await
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty());
        if let Some(sel) = fresh {
            ctx.selection = Some(sel);
        } else if ctx
            .selection
            .as_ref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
        {
            // Nothing selected anymore — fall through as generate instead of failing.
            log::info!("command-edit lost selection; treating as generate");
        }
    }

    let final_text = match mode {
        UtteranceMode::Dictation => {
            match rt.settings.llm.cleanup_level {
                CleanupLevel::None => raw_transcript.clone(),
                level => {
                    let dict_words = dictionary_words_ordered(&rt.store);
                    let prompt_ctx = PromptContext {
                        app_name: ctx.app_name.clone(),
                        field_context: tail_chars(&ctx.field_context, FIELD_CONTEXT_MAX_CHARS),
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
                            let first = match chat_completion(
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
                            };
                            // Safeguard: dictation cleans the transcript, it never
                            // rewrites the document. Strip any echoed field context;
                            // if the output still looks like a document rewrite,
                            // redo the call without context (fallback: raw words).
                            let stripped =
                                strip_context_echo(&first, &prompt_ctx.field_context);
                            let suspicious = stripped.trim().is_empty()
                                || dictation_output_suspicious(
                                    &stripped,
                                    &prompt_ctx.field_context,
                                    &raw_transcript,
                                );
                            if !suspicious {
                                stripped
                            } else {
                                log::warn!(
                                    "dictation output echoed field context; retrying without context"
                                );
                                let bare_ctx = PromptContext {
                                    field_context: String::new(),
                                    ..prompt_ctx.clone()
                                };
                                let retried = match build_dictation(
                                    level,
                                    &rt.settings.prompts,
                                    &bare_ctx,
                                    &raw_transcript,
                                ) {
                                    Some(m2) => chat_completion(
                                        &m2.system,
                                        &m2.user,
                                        &rt.settings.llm.model,
                                        &rt.settings.llm.api_key,
                                    )
                                    .await
                                    .unwrap_or_else(|_| raw_transcript.clone()),
                                    None => raw_transcript.clone(),
                                };
                                // Still strip any residual echo; fall back to raw STT if empty.
                                let cleaned = strip_context_echo(&retried, &prompt_ctx.field_context);
                                if cleaned.trim().is_empty() {
                                    raw_transcript.clone()
                                } else {
                                    cleaned
                                }
                            }
                        }
                    }
                }
            }
        }
        UtteranceMode::CommandEdit | UtteranceMode::CommandGenerate => {
            let dict_words = dictionary_words_ordered(&rt.store);
            let prompt_ctx = PromptContext {
                app_name: ctx.app_name.clone(),
                field_context: tail_chars(&ctx.field_context, FIELD_CONTEXT_MAX_CHARS),
                dictionary: dict_words,
            };
            let selection = ctx
                .selection
                .clone()
                .filter(|s| !s.trim().is_empty());
            // Edit only when we started as edit *and* still have selection text.
            let msgs = match (mode, &selection) {
                (UtteranceMode::CommandEdit, Some(sel)) => build_command(
                    &rt.settings.prompts,
                    &prompt_ctx,
                    &raw_transcript,
                    sel,
                ),
                _ => build_command_generate(
                    &rt.settings.prompts,
                    &prompt_ctx,
                    &raw_transcript,
                ),
            };
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

    // Empty model output — do not wipe the target field.
    if final_text.trim().is_empty() {
        inject::force_release_all_modifiers();
        overlay::show_toast(&rt.overlay_tx, "Empty result — nothing inserted");
        rt.set_state(EngineState::Idle);
        return;
    }

    // Command-edit: if selection is gone at inject time, warn but still insert.
    let had_edit_selection = mode == UtteranceMode::CommandEdit
        && ctx
            .selection
            .as_ref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
    if had_edit_selection {
        let still = tokio::task::spawn_blocking(context::read_selection_with_fallback)
            .await
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty());
        if still.is_none() {
            overlay::show_toast(
                &rt.overlay_tx,
                "Selection lost — inserting at cursor",
            );
            // Brief pause so the toast is readable; inject proceeds.
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        }
    }

    rt.set_state(EngineState::Injecting);

    let method = rt.settings.output.injection_method;
    let restore = rt.settings.output.restore_clipboard;
    let inject_text = final_text.clone();
    let inject_result =
        tokio::task::spawn_blocking(move || inject::inject_text(&inject_text, method, restore))
            .await;

    match inject_result {
        Ok(Ok(inject::InjectOutcome::External)) => {}
        Ok(Ok(inject::InjectOutcome::InApp)) => {
            // Scratchpad / settings WebView — deliver via shell frontend event.
            log::info!(
                "emitting AppInsert ({} chars) for in-process window",
                final_text.chars().count()
            );
            rt.emit(EngineEvent::AppInsert {
                text: final_text.clone(),
            });
            // Give the WebView time to apply the insert before we finish.
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        }
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
    // `EngineEvent::Injected.words` is `u32` (vf-core); SQLite `HistoryEntry.word_count` is `i64`.
    let words_u32 = word_count(&final_text);
    let words_i64 = i64::from(words_u32);

    let history_mode = match mode {
        UtteranceMode::Dictation => "dictation",
        UtteranceMode::CommandEdit => {
            // If we fell back to generate (no selection for LLM), label accordingly.
            if ctx
                .selection
                .as_ref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
            {
                "command"
            } else {
                "command_generate"
            }
        }
        UtteranceMode::CommandGenerate => "command_generate",
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
        word_count: words_i64,
    };
    if let Err(e) = rt.store.history_append(&entry) {
        log::warn!("history_append failed: {e}");
    }

    rt.emit(EngineEvent::Injected {
        words: words_u32,
        total_ms,
    });

    // Auto-learn + use_count bump (§15). Pin HWND from utterance start.
    autolearn::spawn_auto_learn(
        rt.store.clone(),
        final_text,
        ctx.hwnd,
        rt.settings.dictionary.auto_learn,
        rt.event_tx.clone(),
    );

    // Belt-and-suspenders: never leave modifiers stuck after an utterance.
    inject::force_release_all_modifiers();
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
