# GrokBuild brief — owns vf-cloud, vf-engine (2 sessions)

Run one session block per phase, in order. Prereq for both: Antigravity's P1 commit exists (vf-core types available). Read `CONTRACTS.md` fully first; it wins over everything.

---

## SESSION 1 (P2) — crates/vf-cloud

Scope: ElevenLabs realtime STT client, Groq client, prompt builder. Everything per CONTRACTS §6–§9.

1. **STT client** (§6): async (tokio + tokio-tungstenite). API: open a session for one utterance → feed PCM chunks (16 kHz mono s16le, base64) → `commit()` on push-to-talk release → resolve to the committed transcript. Expose partial transcripts as an optional callback/stream for overlay preview. Config from `Settings.stt` + keyterms built per §6 from dictionary entries passed in by the caller.
   - ⚠ First, confirm the exact wire JSON field names from https://elevenlabs.io/docs/api-reference/speech-to-text/v-1-speech-to-text-realtime (you have web access). Everything else (endpoint, auth header, params, message/error types) is already verified in §6.
   - Key rotation exactly per §6 (ordered keys, rotate on auth/quota/rate errors, buffer + resend audio on mid-utterance reconnect, one full cycle max, aggregate error after).
2. **Groq client** (§7): `chat_completion(system, user, model, api_key) -> String` with §7 params and response cleanup; `list_models(api_key) -> Vec<String>`.
3. **Prompt builder** (§8–§9): given cleanup level or command mode, live prompt texts from `Settings.prompts`, plus `app_name` / `field_context` / dictionary words / `instruction` / `selection` → produce the exact (system, user) message pair per §9's message shapes and placeholder rules.
4. Unit tests: prompt builder placeholder resolution (incl. empty → `(none)`), key-rotation state machine (mock transport), Groq response parsing. No network in tests.
5. `cargo build --workspace` + `cargo clippy` clean, tests green, commit: `grokbuild: P2: vf-cloud`.

Constraints: §16 rules; §3 crate whitelist; do NOT touch vf-core, vf-store, app/, or vf-engine. Public API should be caller-friendly for vf-engine (you write that next session).

---

## SESSION 2 (P3) — crates/vf-engine

Scope: the Windows system layer + orchestrator. Everything per CONTRACTS §5, §12, §15.

1. `vf_engine::spawn(settings, store) -> EngineHandle` (§12): engine thread owning a tokio runtime; `EngineCmd` channel; `EngineEvent` broadcast.
2. **Hotkeys** (§5): `WH_KEYBOARD_LL` hook thread; parse combo strings from settings (`Ctrl+Shift+Z` etc.); push-to-talk = act on key-down, finish on key-up; swallow matched combo events; re-arm on `ApplySettings`.
3. **Audio** (§5): cpal capture; resolve current Windows default device at each utterance start when `input_device == "system_default"`; convert/resample to 16 kHz mono s16le (rubato allowed if needed); RMS level for the overlay pulse.
4. **Context reader** (§5): UI Automation via `windows` crate — focused element text near caret (cap ~1500 chars), selected text (for Command Mode), window title + process exe name. All best-effort with graceful `None`.
5. **Injection** (§5, §10): `clipboard_paste` = save clipboard (arboard) → set text → SendInput Ctrl+V → short settle delay → restore when `restore_clipboard`; `sendinput_typing` = KEYEVENTF_UNICODE stream. Target app keeps focus throughout.
6. **Overlay** (§5): Win32 layered pill window (TOPMOST | NOACTIVATE | TOOLWINDOW), bottom-center; states "Recording" (+ level pulse) / "Processing" / brief error toast; hidden when Idle. Simple text + minimal animation only.
7. **Orchestrator** (§5): full dictation flow 1–5 and Command Mode flow using vf-cloud; cleanup `none` skips LLM; history rows appended via `Store`; scratchpad hotkey → `ToggleScratchpad` event; errors per §5.
8. **Auto-learn** (§15) exactly as specified, including `bump_use_count`.
9. Provide `examples/headless.rs`: runs the engine from a settings file for manual testing without the Tauri app (prints events to stdout).
10. `cargo build --workspace` + clippy clean, commit: `grokbuild: P3: vf-engine`.

Constraints: §16 rules; §3 whitelist; do NOT touch vf-core, vf-store, vf-cloud (consume their public APIs; if an API is missing/awkward, note it in your summary — do not modify their crates), or app/.
