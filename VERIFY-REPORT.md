# VillFlow Verification Report

## V1 — 2026-07-13
build: PASS   clippy: PASS (0 warnings)   tests: PASS (2 tests)
findings:
- [minor] crates/vf-core/Cargo.toml, crates/vf-store/Cargo.toml: dependency `chrono` is outside the CONTRACTS §3 allowed-crate whitelist. Used for RFC3339 timestamps (dictionary.created_at, history.ts, scratchpad.updated_at, insights 365-day cutoff). Flagging per §16 rule 2; not fixed (removing chrono exceeds the 30-line budget and would alter timestamp behavior).
- [minor] crates/vf-cloud/src/lib.rs, crates/vf-engine/src/lib.rs: P1 added 1-line stub files (`// GrokBuild owns this crate.`) in crates owned by another agent (§4). Necessary for `cargo build --workspace` to compile the empty members; files are clearly marked and contain no logic, so no ownership conflict at the code level.
- [info] No `// TODO(vf)` markers present in the P1 source diff.
- [info] Settings schema (§10), SQLite schema (§11), and vf-core surface (§12: enums, Settings, EngineEvent/Cmd/State, DictEntry, HistoryEntry, InsightsSummary, Store trait, default prompt consts, default_settings) all match the contract. No API keys logged; no §2 non-goals; no telemetry.
fixes applied: none (build, clippy, and tests already clean — no compile/clippy errors, typos, or missing derives to correct within budget).

## V2 — 2026-07-13
build: PASS   clippy: PASS (0 warnings)   tests: PASS (31 tests: 29 vf-cloud + 2 vf-store)
findings:
- [info] crates/vf-cloud/src/stt.rs: §6 warned the exact ElevenLabs wire field names were not pinned at contract time. P2 pins them (message_type, audio_base_64, input_audio_chunk, session_started/partial_transcript/committed_transcript(+_with_timestamps), auth_error/quota_exceeded/rate_limited/resource_exhausted) and documents confirmation from the ElevenLabs docs at stt.rs:3-4. Per CONTRACTS §6, re-verify against live docs during P3 integration; if any field name contradicts, record it in VERIFY-REPORT and stop — do not improvise.
- [info] Key rotation (KeyRotator) correctly enforces "at most one full cycle per utterance" and buffers audio for mid-utterance resend on rotatable errors; HTTP 401/403/429 at handshake also trigger rotation. Matches §6.
- [info] prompt.rs correctly returns None for CleanupLevel::None (LLM skipped, §8) and builds the exact §9 (system, user) shapes, including command INSTRUCTION:/TEXT: formatting and `(none)` substitution for empties. keyterms.rs enforces 50-term / 20-char caps with starred-first then use_count ordering (§6).
- [info] groq.rs matches §7: Bearer auth, temperature 0.2, max_completion_tokens 2048, non-streaming, choices[0].message.content trimmed + quote/fence stripping, GET /openai/v1/models → data[].id. API key is never logged (error snippets truncated, key never included).
- [info] No `// TODO(vf)` markers in P2 source. Only the GrokBuild-owned `vf-cloud` crate (plus Cargo.lock) was touched — crate-ownership §4 respected. All dependencies are within the §3 whitelist (tokio, tokio-tungstenite, futures-util, reqwest, serde, serde_json, thiserror, anyhow, base64, log). No §2 non-goals, no telemetry, no API keys logged.
fixes applied: none (build, clippy, and tests already clean — nothing to correct within the 30-line budget).

## V3 — 2026-07-13
build: PASS   clippy: PASS (0 warnings)   tests: PASS (36 tests: 29 vf-cloud + 5 vf-engine + 2 vf-store)
findings:
- [info] crates/vf-engine/src/util.rs deliberately avoids `chrono` (not on the §3 whitelist — see V1 finding) by using `windows::GetLocalTime` for the history `ts`. Good: the engine crate stays within the whitelist. (vf-core/vf-store still use `chrono`; that V1 finding stands and is untouched by P3.)
- [info] §5 implemented: `WH_KEYBOARD_LL` hook with key-up detection + event swallowing (hotkeys.rs); cpal capture resolving the current default input device per utterance, resampled to 16 kHz mono s16le via rubato (audio.rs); UIA context read capped at 1500 chars + Ctrl+C clipboard save/restore selection fallback (context.rs); Win32 layered overlay bottom-center, never takes focus, hidden when Idle (overlay.rs); clipboard-paste / sendinput-typing injection with restore_clipboard (inject.rs). Orchestrator state machine Idle→Recording→Processing→Injecting→Idle with Dictation + Command flows, "Select text first" abort, key rotation via vf-cloud, Groq cleanup (None skips LLM), history append, and auto-learn (orchestrator.rs).
- [info] §15 implemented: auto-learn waits ~8s, re-reads focused text, word-aligns injected vs current, accepts single-token edits with distance 1–3, token length ≥4, non-stopword, max 3/utterance, plus use_count bump (autolearn.rs). All rules present and unit-tested.
- [info] Dependency set is within the §3 whitelist (tokio, futures-util, cpal, rubato, arboard, serde, serde_json, thiserror, anyhow, log, dirs, windows, env_logger as dev-dep, vf-store as dev-dep). No extra crates added.
- [info] Only the GrokBuild-owned `vf-engine` crate (+ Cargo.lock and an example) was touched — §4 ownership respected. No `// TODO(vf)` markers. No §2 non-goals, no telemetry, no API keys logged.
- [minor/cosmetic] crates/vf-engine/src/overlay.rs:318 has a no-op `let _ = (GetDC, ReleaseDC);` to silence an unused-import warning. Harmless; not a contract issue. Could be removed by dropping the imports, but not necessary.
fixes applied: none (build, clippy, and tests already clean — nothing to correct within the 30-line budget).

## V4 — 2026-07-13
build: PASS   clippy: PASS (0 warnings)   tests: PASS (36 tests)   npm run build (app/ui): PASS
findings:
- [info] Only Antigravity-owned crates touched (app/src-tauri, app/ui, plus minor additions to vf-core/vf-store which are also Antigravity-owned): `dictionary_update` was added to the `Store` trait (vf-core) and implemented (vf-store) to back the §13 `dictionary_update` IPC command. Consistent and within ownership §4.
- [info] Dependencies added to app/src-tauri (vf-cloud, cpal, winreg) are all within the §3 whitelist. cpal/winreg explicitly allowed.
- [info] §13 IPC commands — all 16 present and names match exactly: get_settings, save_settings, list_groq_models, list_input_devices (returns "system_default" pseudo-entry matching §10), dictionary_list/add/update/delete/toggle_star, history_list(limit,offset), insights_summary, scratchpad_get/set(content), reset_prompt(name)→vf-core default, set_autostart/autostart_status (HKCU …\Run value VillFlow = exe path via winreg). All are invoked from the UI (main.ts / scratchpad.ts).
- [info] §14 UI windows — main window has all 10 nav sections (General, Dictation, Hotkeys, Dictionary, AI Services, Prompts, Output, History, Insights, About) and a separate always-on-top Scratchpad window (scratchpad.html). Plain dark theme, system font.
- [todo-stub] app/src-tauri/src/main.rs:24 — `save_settings` has `// TODO(vf): wired in P5 - EngineCmd::ApplySettings`. This is the engine-boundary stub explicitly permitted by the Antigravity brief (engine host lands in P5); persistence via vf-store works. Listed per §16 rule 3.
- [minor] `autostart_status` IPC command is defined and registered but not invoked by the frontend (the UI derives the "launch at startup" checkbox from `settings.json`, and `set_autostart` is called on save). Not a contract violation — the command exists and is usable — but it is currently dead from the UI's perspective.
- [info] `npm run build` in app/ui succeeds (vite build, dist emitted). No `// TODO(vf)` markers elsewhere. No §2 non-goals, no telemetry, no API keys logged (list_groq_models reads the key but never returns or logs it).
fixes applied: none (build, clippy, tests, and npm build all clean — nothing to correct within the 30-line budget).

## V5 — 2026-07-13
build: PASS   clippy: PASS (0 warnings)   tests: PASS (36 tests)   npm run build (app/ui): PASS   cargo build --release --workspace: PASS
findings:
- [info] Engine host integrated (§5/§13): `vf_engine::spawn` is called at startup, the `EngineHandle` is managed as Tauri state, and `save_settings` now builds `EngineCmd::ApplySettings(Box::new(settings))` and sends it to the engine. This resolves the V4 `// TODO(vf)` stub (no remaining `// TODO(vf)` markers in source).
- [info] Tray (§14): menu = Open VillFlow / Scratchpad / Quit; V3 icon (icons/32x32.png); tooltip reflects engine state (Idle/Recording/Processing/Injecting); left-click shows the main window. CloseRequested is intercepted to hide-to-tray (app keeps running). `start_minimized` honored. All match §14.
- [info] Notifications (§5): `tauri-plugin-notification` (within the §3 whitelist) drives `EngineEvent::Error` → tray tooltip + Windows notification when `general.show_error_notifications` is true. capabilities/default.json grants `core:default` + `notification:default` (covers frontend event listening).
- [info] Autostart: `set_autostart`/`autostart_status` persist/read HKCU `…\Run` value `VillFlow` = exe path (wired in P4, exercised here).
- [info] Only the Antigravity-owned `app` crate was touched (main.rs, Cargo.toml, capabilities). Dependencies added (`tauri-plugin-notification`, `vf-engine`, tauri features `tray-icon`/`image-png`) are within the §3 whitelist.
- [minor] §13 lists `scratchpad-toggle` as a shell→frontend event. The bridge consumes `EngineEvent::ToggleScratchpad` internally (toggling the scratchpad Tauri window directly) and does NOT emit a `scratchpad-toggle` event to the frontend. `engine-state` and `engine-error` are emitted. Reasonable since the scratchpad is a shell-owned window, but the named frontend event is not forwarded — report-only.
- [info] No §2 non-goals, no telemetry, no API keys logged (notification body uses the error message only; settings/keys never logged).
fixes applied: 1 line — clippy `clone_on_copy` warning at app/src-tauri/src/main.rs:265 (`state.clone()` → `state`, since `EngineState` is `Copy`). This satisfies the §16 rule 2 "clippy should be clean" requirement and is within the 30-line budget.

## V6 — 2026-07-13 (orchestrator, post-V5 launch failure + live validation)
symptom: launching villflow.exe showed the WebView error page "localhost refused to connect (ERR_CONNECTION_REFUSED)".
root cause: the app was compiled with plain `cargo build --release`, which does NOT enable Tauri's production asset embedding — the window loads `build.devUrl` (http://localhost:5173) unless the app is built through the Tauri CLI (`tauri build`), which enables the production feature and embeds `frontendDist`. V5 verified the cargo build compiled but never launched the exe, so this passed silently.
fixes applied:
- app/src-tauri/tauri.conf.json: `beforeDevCommand` = `npm run dev` and `beforeBuildCommand` = `npm run build` (cwd ../ui), so CLI dev/build flows are self-contained.
- app/ui/package.json: added `@tauri-apps/cli` ^2 as devDependency (no cargo-tauri subcommand was installed on this machine).
- canonical production build is now: `ui\node_modules\.bin\tauri.cmd build --no-bundle` run from `app\` (documented in README.md). Verified: exe now renders the full UI (all 10 sections) from embedded assets.
- crates/vf-cloud/examples/live_smoke.rs: new live smoke test (reads real keys from %APPDATA%\VillFlow\settings.json).
live validation results (real keys, 2026-07-13):
- Groq: 17 models listed in 159ms; default model `openai/gpt-oss-120b` present; chat completion round-trip 311ms.
- ElevenLabs realtime: session opened, partial transcripts streamed live, committed final transcript 258ms after last chunk — empirically confirms the §6 wire schema pinned in P2 (message_type / audio_base_64 / commit / partial_transcript / committed_transcript).
- Engine: spawns at app startup; VillFlowOverlayClass window present and hidden at Idle, per §5.
open observations (report-only, not fixed):
- [minor] About page shows "Version 3.0.0 (MSVC Build)" / footer "3.0.0-p4" while tauri.conf.json says 0.1.0 — cosmetic version drift invented by the UI, not from spec.
- [minor] Tauri CLI warns the identifier `com.villflow.app` ends in `.app` (conflicts with macOS bundle extension). Irrelevant for Windows; revisit before any Mac build.
- [minor] `npm audit`: 2 advisories (1 moderate, 1 high) in the vite 5.x dev-dependency tree; dev-time only, consider bumping vite later.

## Product drift — command mode without selection (2026-07-13)

CONTRACTS §5 / §18 still say Command Mode requires a selection and aborts with
"Select text first" when none is present. Product behavior (user request) now
allows no-selection command mode: spoken instruction generates new content at
the cursor (`PROMPT_COMMAND_GENERATE` / history `mode=command_generate`).
With a selection, behavior remains transform-and-replace (`mode=command`).
Do not "fix" code back to the old contract without updating CONTRACTS.

## V7 — 2026-07-13 (full audit fix pass)

build: PASS   clippy: PASS (`--all-targets -D warnings`)   tests: PASS (44: 32 vf-cloud + 3 vf-core + 7 vf-engine + 2 vf-store)   npm run build: PASS

Critical/high fixes applied:
- hotkeys: bare Z/X/C key-up no longer swallowed system-wide; hook install signals success/failure; `hotkeys::shutdown()` on engine Shutdown; tray Quit sends `EngineCmd::Shutdown`
- STT: `open` awaits handshake; connect/commit timeouts; pre-commit errors preserved; send-fail reconnect; word-boundary HTTP status parse; buffer until `session_started`
- Groq: shared client with connect + request timeouts
- settings: corrupt JSON → backup + defaults; insights daily_words dictation-only; dictionary validation/NOCASE index/deduped bump; relative-date store tests; clippy bool asserts fixed
- engine: duration from key-down; empty transcript abort; command re-check selection; auto-learn HWND pin + better align; audio buffer/flush; caret-near field context (tail cap); headless Ctrl+C shutdown
- UI/app: `system_default` wire value; autostart quoted path + save-then-registry; local heatmap dates; version 0.1.0; system fonts; scratchpad allowlist sanitize; start_minimized no flash (`visible: false`)

Remaining known (not fully eliminable here):
- production UI still requires Tauri CLI build (not plain `cargo build`) — documented in README
- npm audit vite/esbuild dev advisories (dev-only)
- full live PTT e2e not re-run in this pass (unit/integration + previous V6 live smoke)
