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
    partial_task: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UtteranceMode {
    Dictation,
    /// Transform selected text.
    CommandEdit,
    /// Generate new content (no selection).
    CommandGenerate,
}

impl UtteranceMode {
    fn overlay_label(self) -> &'static str {
        match self {
            Self::Dictation => "Recording",
            Self::CommandEdit => "Edit",
            Self::CommandGenerate => "Generate",
        }
    }
}

/// PRODUCT.md: refuse immediately when keys are missing — no fake Recording.
fn check_ready(settings: &Settings, mode: UtteranceMode) -> Result<(), String> {
    let has_el = settings
        .stt
        .api_keys
        .iter()
        .any(|k| !k.trim().is_empty());
    if !has_el {
        return Err("Add your API keys in Setup.".into());
    }
    let needs_groq = match mode {
        UtteranceMode::Dictation => settings.llm.cleanup_level != CleanupLevel::None,
        UtteranceMode::CommandEdit | UtteranceMode::CommandGenerate => true,
    };
    if needs_groq && settings.llm.api_key.trim().is_empty() {
        return Err("Add your Groq API key in Setup.".into());
    }
    Ok(())
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
            if let Some(t) = active.partial_task.take() {
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
                        ) {
                            rt.emit(EngineEvent::Error(format!("hotkey update failed: {e}")));
                        }
                    }
                }
            }

            ev = hotkey_rx.recv() => {
                match ev {
                    None => break,
                    Some(HotkeyEvent::Down(HotkeyId::Dictation)) => {
                        if rt.state == EngineState::Idle {
                            if let Err(msg) = check_ready(&rt.settings, UtteranceMode::Dictation) {
                                inject::force_release_all_modifiers();
                                overlay::show_toast(&rt.overlay_tx, &msg);
                                rt.emit(EngineEvent::Error(msg));
                            } else {
                                begin_utterance(&mut rt, UtteranceMode::Dictation).await;
                            }
                        }
                    }
                    Some(HotkeyEvent::Up(HotkeyId::Dictation)) => {
                        if rt.request_finish_if_matching(UtteranceMode::Dictation) {
                            finish_utterance(&mut rt).await;
                        }
                    }
                    Some(HotkeyEvent::Down(HotkeyId::CommandMode)) => {
                        if rt.state == EngineState::Idle {
                            // Peek mode for readiness (Generate still needs Groq).
                            if let Err(msg) =
                                check_ready(&rt.settings, UtteranceMode::CommandGenerate)
                            {
                                inject::force_release_all_modifiers();
                                overlay::show_toast(&rt.overlay_tx, &msg);
                                rt.emit(EngineEvent::Error(msg));
                            } else {
                                // Selection is optional: with one → Edit; without → Generate.
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
    // Align Setup pill + overlay: Connecting until STT session is ready.
    rt.set_state(EngineState::Connecting);
    overlay::show_connecting(&rt.overlay_tx);

    let overlay_label = mode.overlay_label().to_string();
    let device = rt.settings.audio.input_device.clone();
    let stt_settings = rt.settings.stt.clone();

    let keyterms = match rt.store.dictionary_list() {
        Ok(entries) => build_keyterms(&entries),
        Err(e) => {
            log::warn!("dictionary_list failed: {e}");
            Vec::new()
        }
    };

    // PRODUCT A1: start mic immediately; open STT in parallel; buffer PCM until STT ready.
    let (audio_tx, mut audio_rx) = mpsc::unbounded_channel::<AudioChunk>();
    let capture = match audio::start_capture(&device, audio_tx) {
        Ok(c) => Some(c),
        Err(e) => {
            rt.error(format!("audio capture failed: {e}"));
            return;
        }
    };

    // Session slot starts empty; feed task buffers until open completes.
    let stt: Arc<Mutex<Option<SttSession>>> = Arc::new(Mutex::new(None));
    let stt_for_feed = stt.clone();
    let session_ready = Arc::new(tokio::sync::Notify::new());
    let session_ready_feed = session_ready.clone();
    let overlay_tx = rt.overlay_tx.clone();
    let label_for_feed = overlay_label.clone();
    // Latest RMS for partial preview updates.
    let last_rms = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let last_rms_feed = last_rms.clone();

    let feed_task = tokio::spawn(async move {
        let mut pending: Vec<Vec<u8>> = Vec::new();
        let mut live = false;

        async fn flush_pending(
            stt: &Arc<Mutex<Option<SttSession>>>,
            pending: &mut Vec<Vec<u8>>,
        ) {
            for pcm in pending.drain(..) {
                let guard = stt.lock().await;
                if let Some(session) = guard.as_ref() {
                    if let Err(e) = session.feed_pcm(&pcm).await {
                        log::warn!("STT feed error: {e}");
                    }
                }
            }
        }

        loop {
            tokio::select! {
                chunk = audio_rx.recv() => {
                    let Some(chunk) = chunk else { break; };
                    last_rms_feed.store(chunk.rms.to_bits(), std::sync::atomic::Ordering::Relaxed);
                    if !live {
                        let has = stt_for_feed.lock().await.is_some();
                        if has {
                            flush_pending(&stt_for_feed, &mut pending).await;
                            live = true;
                        } else {
                            // Stay on Connecting… until STT is ready (do not flip to Recording).
                            pending.push(chunk.pcm_s16le);
                            const MAX_PENDING_CHUNKS: usize = 500;
                            if pending.len() > MAX_PENDING_CHUNKS {
                                let drop_n = pending.len() - MAX_PENDING_CHUNKS;
                                pending.drain(0..drop_n);
                            }
                            continue;
                        }
                    }
                    overlay::show_active(&overlay_tx, &label_for_feed, chunk.rms);
                    let guard = stt_for_feed.lock().await;
                    if let Some(session) = guard.as_ref() {
                        if let Err(e) = session.feed_pcm(&chunk.pcm_s16le).await {
                            log::warn!("STT feed error: {e}");
                        }
                    } else {
                        break;
                    }
                }
                _ = session_ready_feed.notified() => {
                    if !live && stt_for_feed.lock().await.is_some() {
                        flush_pending(&stt_for_feed, &mut pending).await;
                        live = true;
                    }
                }
            }
        }
    });

    // Parallel: focus context + STT open.
    let context_fut = tokio::task::spawn_blocking(context::read_focus_context);
    let stt_fut = SttSession::open(stt_settings, keyterms);

    let (context_res, session_res) = tokio::join!(context_fut, stt_fut);
    let context = context_res.unwrap_or_default();

    let session = match session_res {
        Ok(s) => s,
        Err(e) => {
            if let Some(c) = capture {
                c.stop();
            }
            feed_task.abort();
            rt.error(format!("STT open failed: {e}"));
            return;
        }
    };

    let mut partial_rx = session.subscribe_partials();
    // STT ready → Recording first (Setup pill), then unlock feed + overlay.
    rt.set_state(EngineState::Recording);
    {
        let mut guard = stt.lock().await;
        *guard = Some(session);
    }
    session_ready.notify_one();
    overlay::show_active(&rt.overlay_tx, &overlay_label, 0.0);

    // Partial STT preview on overlay (C7).
    let overlay_partial = rt.overlay_tx.clone();
    let label_partial = overlay_label.clone();
    let rms_partial = last_rms.clone();
    let partial_task = tokio::spawn(async move {
        while let Ok(text) = partial_rx.recv().await {
            let t = text.trim();
            if t.is_empty() {
                continue;
            }
            let level = f32::from_bits(rms_partial.load(std::sync::atomic::Ordering::Relaxed));
            overlay::show_active_with_preview(&overlay_partial, &label_partial, level, t);
        }
    });

    rt.active = Some(ActiveUtterance {
        mode,
        started,
        context,
        stt,
        capture,
        feed_task: Some(feed_task),
        partial_task: Some(partial_task),
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
    // Speech window ends when capture stops (key-up) — used for WPM (D6).
    let speech_ms = active.started.elapsed().as_millis() as i64;
    if let Some(c) = active.capture.take() {
        c.stop();
    }
    if let Some(t) = active.partial_task.take() {
        t.abort();
    }
    if let Some(t) = active.feed_task.take() {
        // Brief drain so last PCM reaches STT; don't sit on the happy path long (A4).
        let _ = tokio::time::timeout(std::time::Duration::from_millis(80), t).await;
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

    // For command-edit: prefer a refreshed selection if still available (user
    // adjusted mid-hold). If re-read fails, keep the selection captured at
    // key-down and stay on Edit (PRODUCT: do not demote to Generate).
    let mut effective_mode = mode;
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
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
        {
            log::info!("command-edit: re-read empty; using selection from key-down");
        } else {
            // Started as Edit without stored selection (shouldn't happen) — Generate.
            effective_mode = UtteranceMode::CommandGenerate;
            log::warn!("command-edit had no selection; falling back to generate");
            overlay::show_toast(
                &rt.overlay_tx,
                "No selection — generating at cursor",
            );
        }
    }

    let t_stt_done = Instant::now();
    let final_text = match effective_mode {
        UtteranceMode::Dictation => {
            match rt.settings.llm.cleanup_level {
                CleanupLevel::None => raw_transcript.clone(),
                level => {
                    let dict_words = dictionary_words_ordered(&rt.store);
                    // PRODUCT: field context off by default; optional advanced toggle.
                    let field_context = if rt.settings.llm.include_field_context {
                        tail_chars(&ctx.field_context, FIELD_CONTEXT_MAX_CHARS)
                    } else {
                        String::new()
                    };
                    let prompt_ctx = PromptContext {
                        app_name: ctx.app_name.clone(),
                        field_context: field_context.clone(),
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
                                rt.settings.llm.max_completion_tokens,
                            )
                            .await
                            {
                                Ok(text) => text,
                                Err(e) => {
                                    let err_s = e.to_string();
                                    // Free-tier TPM: retry once at 1024 before falling back to raw.
                                    let retry = if err_s.contains("413")
                                        || err_s.contains("too large")
                                        || err_s.contains("TPM")
                                    {
                                        log::warn!(
                                            "Groq 413/TPM on dictation — retrying with 1024 max tokens"
                                        );
                                        chat_completion(
                                            &msgs.system,
                                            &msgs.user,
                                            &rt.settings.llm.model,
                                            &rt.settings.llm.api_key,
                                            1024,
                                        )
                                        .await
                                        .ok()
                                    } else {
                                        None
                                    };
                                    if let Some(text) = retry {
                                        text
                                    } else {
                                        // Never swallow STT: paste raw if cleanup fails.
                                        log::error!(
                                            "Groq failed (using raw transcript): {err_s}"
                                        );
                                        overlay::show_toast(
                                            &rt.overlay_tx,
                                            "Cleanup failed — pasted raw transcript",
                                        );
                                        rt.emit(EngineEvent::Error(format!(
                                            "Groq failed (raw used): {err_s}"
                                        )));
                                        raw_transcript.clone()
                                    }
                                }
                            };
                            // Light safeguards without a mandatory second Groq call (A4 / B3).
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
                            } else if !field_context.is_empty() {
                                // Context was on and output looks like a rewrite — use raw STT.
                                log::warn!(
                                    "dictation output suspicious with field context; using raw transcript"
                                );
                                raw_transcript.clone()
                            } else if stripped.trim().is_empty() {
                                raw_transcript.clone()
                            } else {
                                stripped
                            }
                        }
                    }
                }
            }
        }
        UtteranceMode::CommandEdit | UtteranceMode::CommandGenerate => {
            let dict_words = dictionary_words_ordered(&rt.store);
            let field_context = if rt.settings.llm.include_field_context
                || effective_mode == UtteranceMode::CommandGenerate
            {
                // Generate may use document context as reference (PROMPT_COMMAND_GENERATE).
                tail_chars(&ctx.field_context, FIELD_CONTEXT_MAX_CHARS)
            } else {
                String::new()
            };
            let prompt_ctx = PromptContext {
                app_name: ctx.app_name.clone(),
                field_context,
                dictionary: dict_words,
            };
            let selection = ctx
                .selection
                .clone()
                .filter(|s| !s.trim().is_empty());
            // Edit only when we still have selection text after refresh.
            let msgs = match (effective_mode, &selection) {
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
                rt.settings.llm.max_completion_tokens,
            )
            .await
            {
                Ok(text) => text,
                Err(e) => {
                    // Command needs LLM; one retry at smallest budget if request was too large.
                    let err_s = e.to_string();
                    if err_s.contains("413") || err_s.contains("too large") || err_s.contains("TPM")
                    {
                        log::warn!("Groq 413/TPM — retrying command with 1024 max tokens");
                        match chat_completion(
                            &msgs.system,
                            &msgs.user,
                            &rt.settings.llm.model,
                            &rt.settings.llm.api_key,
                            1024,
                        )
                        .await
                        {
                            Ok(text) => text,
                            Err(e2) => {
                                rt.error(format!("Groq failed: {e2}"));
                                return;
                            }
                        }
                    } else {
                        rt.error(format!("Groq failed: {e}"));
                        return;
                    }
                }
            }
        }
    };

    let t_llm_done = Instant::now();

    // Empty model output — do not wipe the target field.
    if final_text.trim().is_empty() {
        inject::force_release_all_modifiers();
        overlay::show_toast(&rt.overlay_tx, "Empty result — nothing inserted");
        rt.set_state(EngineState::Idle);
        return;
    }

    // Command-edit still holding a selection: re-check at inject; warn if gone (PRODUCT).
    let had_edit_selection = effective_mode == UtteranceMode::CommandEdit
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
            // Non-blocking toast — do not delay inject (A4 / PRODUCT).
            overlay::show_toast(
                &rt.overlay_tx,
                "Selection lost — inserting at cursor",
            );
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
            // Settings WebView — deliver via shell frontend event.
            log::info!(
                "emitting AppInsert ({} chars) for in-process window",
                final_text.chars().count()
            );
            rt.emit(EngineEvent::AppInsert {
                text: final_text.clone(),
            });
            // Short settle for WebView apply (A4: keep small).
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
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
    let stt_ms = t_stt_done.saturating_duration_since(started).as_millis();
    let llm_ms = t_llm_done.saturating_duration_since(t_stt_done).as_millis();
    let inject_ms = Instant::now().saturating_duration_since(t_llm_done).as_millis();
    log::info!(
        "utterance timings: total={total_ms}ms stt_phase={stt_ms}ms llm_phase={llm_ms}ms inject_phase={inject_ms}ms mode={effective_mode:?}"
    );

    // `EngineEvent::Injected.words` is `u32` (vf-core); SQLite `HistoryEntry.word_count` is `i64`.
    let words_u32 = word_count(&final_text);
    let words_i64 = i64::from(words_u32);

    let history_mode = match effective_mode {
        UtteranceMode::Dictation => "dictation",
        UtteranceMode::CommandEdit => "command",
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
        speech_ms,
        word_count: words_i64,
    };
    if let Err(e) = rt.store.history_append(&entry) {
        log::warn!("history_append failed: {e}");
    }
    // History retention (E2): purge older than N days when configured.
    let days = rt.settings.general.history_retention_days;
    if days > 0 {
        if let Err(e) = rt.store.history_purge_older_than_days(days) {
            log::warn!("history_purge failed: {e}");
        }
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
