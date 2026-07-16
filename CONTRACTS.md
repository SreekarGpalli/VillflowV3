# VillFlow — Build Contracts v1

Technical build contract for agents and implementers. Read fully before writing code.

**Product behavior precedence:** `docs/PRODUCT.md` (owner-locked decisions) **>** this file **>** agent prompts in `prompts/` **>** original human specs under `docs/internal/` (background only).  
If this file conflicts with `docs/PRODUCT.md`, follow PRODUCT.md and update this file when you touch the area.  
If a fact here contradicts live API behavior, do NOT improvise: record the discrepancy in `docs/internal/VERIFY-REPORT.md` and in your final summary, then stop that sub-task.

Fix tracker: `docs/ISSUES-AND-FIX-PLAN.md`.

---

## 1. Product

Windows 11 push-to-talk voice dictation, tray-resident, open source / free / no accounts. Hold **Ctrl+Shift+Z** in any app → speak → on release, polished text is pasted at the cursor of the focused field. Hold **Ctrl+Shift+X** (Command Mode) → dual mode: **with selection** transform/replace; **without selection** generate new text at the cursor. Overlay labels **Edit** vs **Generate**. English only; speakers have Indian accents. Everything local except two cloud calls: ElevenLabs realtime STT and Groq LLM.

**Latency:** best effort only — **no hard millisecond SLA**. Remove artificial delays in code; typical cloud path may be ~0.5–2s depending on network. Cleanup level `none` skips the LLM entirely (fastest).

**Distribution:** GitHub Releases ship **portable `villflow.exe` and a Windows installer** (Tauri bundle). Production UI is embedded via the default `custom-protocol` feature (`frontendDist`); prefer `tauri build` so the UI is rebuilt automatically.

## 2. Non-goals — DO NOT BUILD

Whisper mode; "backtrack" voice command; Transforms/rewrite presets; undo-AI-edit; snippet library; Styles/tone presets or any style settings; mouse-button trigger; microphone auto-ranking; session recovery; 20-minute-session handling; UI localization; multi-language STT; account system; telemetry; auto-update (manual GitHub releases are fine); streaming text into the target field (always paste the final text once); "communication profile" insights.

## 3. Locked stack

- Rust stable, target `x86_64-pc-windows-msvc`.
- **Tauri v2** app shell hosting the Settings window (Overview, Dictation, History, Insights, etc.). Frontend: **vanilla TypeScript + Vite** (Tauri default template) — no React/Vue/UI-kit dependencies. Simple dark theme, system font stack.
- Flow Bar overlay = **native Win32 layered window** (inside vf-engine), NOT a Tauri window.
- Allowed crates: `windows`, `tauri` v2 (+ `tauri-plugin-notification` if needed), `cpal`, `rubato` (only if resampling needed), `tokio`, `tokio-tungstenite`, `futures-util`, `reqwest` (rustls, json), `rusqlite` (bundled), `serde`, `serde_json`, `thiserror`, `anyhow`, `base64`, `arboard`, `winreg`, `dirs`, `log` + `env_logger`. Anything else: flag it in your summary before adding.
- Data dir: `%APPDATA%\VillFlow\` → `settings.json`, `villflow.db`, `logs\villflow.log`.
- Identity: product **VillFlow**, exe `villflow`, Tauri identifier `com.villflow.app`. Icons: generate the full set from `assets/icon.png` (plain "V3" lettermark) via `cargo tauri icon assets/icon.png`.

## 4. Workspace layout & ownership

```
Cargo.toml              workspace root
CONTRACTS.md            this file
assets/icon.png         V3 lettermark source (exists)
crates/vf-core/         [Antigravity]  shared types, settings structs + defaults, events, default prompt consts. Types only — no I/O.
crates/vf-store/        [Antigravity]  settings.json load/save; SQLite (dictionary/history); insights queries. Implements vf-core::Store.
crates/vf-cloud/        [GrokBuild]    ElevenLabs realtime client + key rotation; Groq client + model list; prompt builder.
crates/vf-engine/       [GrokBuild]    global hotkeys, audio capture, UIA context reading, text injection, Win32 overlay, auto-learn, orchestrator state machine.
app/                    [Antigravity]  Tauri shell: src-tauri (IPC commands, tray, engine host) + ui/ (frontend).
prompts/                agent briefs — read only your own file.
```

**Rules:** never edit a crate owned by another agent. Cross-crate wiring happens only in `app/` during the final phase. opencode (verifier) may apply fixes ≤ 30 changed lines per verification pass, anywhere.

## 5. Runtime architecture

Single process (Tauri). Startup: load settings → spawn engine thread (owns a tokio runtime) → `EngineHandle` exposes a command channel in and a broadcast of `EngineEvent` out. Engine states: `Idle → Recording → Processing → Injecting → Idle`, plus transient `Error(String)`.

**Dictation flow (push-to-talk only — no toggle mode):**
1. Hotkey down (full combo matched): swallow the keystrokes (the target app must never receive Ctrl+Shift+Z). If not ready (no ElevenLabs keys, or cleanup≠none and no Groq key): **refuse immediately** with a clear overlay/toast (e.g. “Add your API keys in Setup”) — do **not** enter Recording or open STT.
2. Otherwise: start **microphone capture immediately** (ring-buffer). In parallel, open ElevenLabs WebSocket. If `audio.input_device == "system_default"`, resolve the CURRENT Windows default input device at each utterance start; else use the named device. Resample to 16 kHz mono s16le. Flush buffered PCM when the session is ready, then stream live. Read context best-effort (window title, process name; field text only if advanced “include field context” is on — see PRODUCT.md).
3. Overlay: **Connecting…** while waiting for capture/STT as needed; then **Recording** + minimal level pulse **only when mic is capturing**.
4. Hotkey up: stop capture, send final audio with commit. Overlay shows **Processing**.
5. On committed transcript: `cleanup_level == none` → inject raw text. Otherwise call Groq with the built prompt (§8–§9) → inject the response. Default: **do not** send document field context unless advanced toggle is on.
6. Inject per §Output settings. Overlay hides. Append a history row (word_count = words in final text; duration_ms = key-down→injection). Start the auto-learn watcher (§15) only if `dictionary.auto_learn` is true.

**Command Mode flow (dual mode — PRODUCT.md):**
1. Read selection via UIA; fallback: simulated Ctrl+C with full clipboard save/restore.
2. **With selection** → mode Edit: record spoken instruction, Groq with command prompt (§9), inject over selection. History `mode='command'`. Overlay **Edit** while active.
3. **Without selection** → mode Generate: record spoken instruction, Groq with command-generate prompt, insert at cursor. History `mode='command_generate'`. Overlay **Generate**.
4. If Edit started with a selection but selection is gone at inject time: **still insert at cursor** + short warning toast (do not abort).



**Hotkeys:** use a `WH_KEYBOARD_LL` low-level keyboard hook — NOT `RegisterHotKey` (need key-up detection for push-to-talk and event swallowing). Combos come from settings and re-arm on settings change. While a combo is engaged, swallow its key events.

**Overlay:** layered window, `WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`, small pill bottom-center of the primary monitor. Text states "Recording" / "Processing" with a minimal pulse animation. Never takes focus. Hidden when Idle.

**Errors:** `EngineEvent::Error` → tray tooltip; plus a Windows notification when `general.show_error_notifications` is true.

## 6. ElevenLabs realtime STT (verified July 2026)

- WebSocket: `wss://api.elevenlabs.io/v1/speech-to-text/realtime` (regional hosts exist, e.g. `api.in.residency.elevenlabs.io`; endpoint host comes from settings, default US).
- Auth: `xi-api-key` header on the handshake.
- Config: `model_id=scribe_v2_realtime`, `audio_format=pcm_16000`, `language_code=en`, `commit_strategy=manual`, `keyterms` = up to 50 terms, each ≤ 20 chars (build from dictionary: starred words first, then highest `use_count`, truncate to limits).
- Client sends `input_audio_chunk` messages (base64 PCM; commit flag on the final one). Server sends `session_started`, `partial_transcript`, `committed_transcript` (+ error events: `auth_error`, `quota_exceeded`, `rate_limited`, `resource_exhausted`, etc.). Use the committed transcript as final; partials may optionally drive an overlay preview but nothing else.
- ⚠ Exact JSON field names were not pinned at contract time. Before implementing, confirm the wire schema at https://elevenlabs.io/docs/api-reference/speech-to-text/v-1-speech-to-text-realtime — do not guess silently.
- **Key rotation ("unlimited fallback"):** `stt.api_keys` is an ordered list of ElevenLabs keys. On `auth_error` / `quota_exceeded` / `rate_limited` / `resource_exhausted` (or HTTP 401/403/429 at handshake): close, advance to the next key (wrap around; at most one full cycle per utterance), reconnect, resume. Keep the utterance's audio buffered until committed so a mid-utterance reconnect can resend it. Only when every key fails: `EngineEvent::Error("All ElevenLabs keys failed: <last error>")`.

## 7. Groq LLM (verified July 2026)

- `POST https://api.groq.com/openai/v1/chat/completions`, `Authorization: Bearer <llm.api_key>`. OpenAI-compatible.
- Model = `llm.model`; shipped default `"openai/gpt-oss-120b"`. Model picker data: `GET https://api.groq.com/openai/v1/models` → list of ids (fetched live by the UI via `list_groq_models`).
- Request: non-streaming, `temperature 0.2`, `max_completion_tokens 8192`. Result = `choices[0].message.content`, trimmed; strip wrapping quotes/code fences if present.

## 8. Cleanup levels

| Level | Behavior |
|---|---|
| `none` | Skip LLM. Inject the raw committed transcript verbatim. |
| `light` | Remove filler words; fix punctuation + capitalization. No other word changes. |
| `medium` (default) | light + grammar fixes + split run-on sentences + format spoken lists as bulleted/numbered lists. |
| `high` | medium + tighten wording for clarity/concision; meaning must be preserved. |

## 9. Default system prompts (consts in vf-core; live values in settings.json; per-prompt reset-to-default)

Placeholder contract — the vf-cloud prompt builder resolves: `{app_name}`, `{field_context}`, `{dictionary}` (comma-separated words, starred first), `{instruction}`, `{selection}`. Empty value → substitute `(none)`.

Message shapes:
- Dictation (light/medium/high): `system` = resolved template below; `user` = the raw transcript, nothing else.
- Command: `system` = resolved PROMPT_COMMAND; `user` = `INSTRUCTION:\n{instruction}\n\nTEXT:\n{selection}` (builder resolves these two placeholders in the user message, not the system message).

**PROMPT_LIGHT:**
```
You clean up raw speech-to-text dictation from an Indian English speaker. Output ONLY the cleaned text — no preamble, no quotes, no explanation. Remove filler words (uh, um, like, you know, actually when meaningless). Fix punctuation and capitalization. Do not change, add, or reorder any other words. Prefer these spellings when the words occur: {dictionary}. The text will be inserted into {app_name}. Existing text before the cursor is shown for continuity — continue from it naturally and never repeat it: {field_context}
```

**PROMPT_MEDIUM:**
```
You clean up raw speech-to-text dictation from an Indian English speaker. Output ONLY the cleaned text — no preamble, no quotes, no explanation. Remove filler words (uh, um, like, you know). Fix grammar, punctuation, and capitalization. Split run-on sentences. If the speaker dictates a list, format it as a bulleted or numbered list. Keep the speaker's meaning and vocabulary — do not add content or embellish. Prefer these spellings when the words occur: {dictionary}. The text will be inserted into {app_name} — match a tone appropriate to that app. Existing text before the cursor is shown for continuity — continue from it naturally and never repeat it: {field_context}
```

**PROMPT_HIGH:**
```
You clean up raw speech-to-text dictation from an Indian English speaker. Output ONLY the cleaned text — no preamble, no quotes, no explanation. Remove filler words. Fix grammar, punctuation, and capitalization. Split run-on sentences. Format spoken lists as bulleted or numbered lists. Tighten the wording for clarity and concision, but preserve the speaker's meaning and intent exactly — never add new information. Prefer these spellings when the words occur: {dictionary}. The text will be inserted into {app_name} — match a tone appropriate to that app. Existing text before the cursor is shown for continuity — continue from it naturally and never repeat it: {field_context}
```

**PROMPT_COMMAND:**
```
You apply a spoken editing instruction to a piece of text. Output ONLY the transformed text — no preamble, no quotes, no explanation. Preserve the original formatting style (line breaks, lists) unless the instruction says otherwise. The text lives in {app_name}. Apply the INSTRUCTION to the TEXT that follows.
```

## 10. Settings schema — `%APPDATA%\VillFlow\settings.json`

```json
{
  "version": 1,
  "general": {
    "launch_at_startup": false,
    "start_minimized": false,
    "show_error_notifications": true
  },
  "hotkeys": {
    "dictation": "Ctrl+Shift+Z",
    "command_mode": "Ctrl+Shift+X"
  },
  "audio": {
    "input_device": "system_default"
  },
  "stt": {
    "api_keys": [],
    "endpoint": "wss://api.elevenlabs.io",
    "model_id": "scribe_v2_realtime",
    "language_code": "en"
  },
  "llm": {
    "api_key": "",
    "model": "openai/gpt-oss-120b",
    "cleanup_level": "medium"
  },
  "prompts": {
    "light": "<PROMPT_LIGHT default>",
    "medium": "<PROMPT_MEDIUM default>",
    "high": "<PROMPT_HIGH default>",
    "command": "<PROMPT_COMMAND default>"
  },
  "output": {
    "injection_method": "clipboard_paste",
    "restore_clipboard": true
  },
  "dictionary": {
    "auto_learn": false
  }
}
```

Missing fields on load → filled from defaults (serde `#[serde(default)]`), then re-saved. `injection_method` ∈ `clipboard_paste | sendinput_typing`. `cleanup_level` ∈ `none | light | medium | high`. API keys live only in this local file — never log them.

**Defaults (PRODUCT.md):** `dictionary.auto_learn` = **false**; default cleanup = **medium**. Advanced setting (implementation may add): include field context for dictation continuity — **off** by default.

## 11. SQLite schema — `%APPDATA%\VillFlow\villflow.db`

```sql
CREATE TABLE IF NOT EXISTS dictionary (
  id INTEGER PRIMARY KEY,
  word TEXT NOT NULL UNIQUE,
  starred INTEGER NOT NULL DEFAULT 0,
  source TEXT NOT NULL DEFAULT 'manual',      -- 'manual' | 'auto'
  use_count INTEGER NOT NULL DEFAULT 0,       -- the "weight"
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS history (
  id INTEGER PRIMARY KEY,
  ts TEXT NOT NULL,                           -- ISO 8601 local
  app_name TEXT NOT NULL,                     -- process exe name
  window_title TEXT NOT NULL DEFAULT '',
  mode TEXT NOT NULL DEFAULT 'dictation',     -- 'dictation' | 'command' | 'command_generate'
  raw_transcript TEXT NOT NULL,
  final_text TEXT NOT NULL,
  duration_ms INTEGER NOT NULL DEFAULT 0,
  word_count INTEGER NOT NULL DEFAULT 0
);
```

Insights are queries over `history` (no extra tables): total words dictated; average WPM = `word_count / (duration_ms/60000)` over dictation rows; top 5 `app_name` by row count; per-day word counts for the last 365 days (streak heatmap).

## 12. vf-core surface (signature level — implementers fill bodies)

```rust
pub enum CleanupLevel { None, Light, Medium, High }
pub enum InjectionMethod { ClipboardPaste, SendInputTyping }

pub struct Settings { /* mirrors §10 exactly, serde Serialize/Deserialize, Default = §10 defaults */ }

pub enum EngineState { Idle, Connecting, Recording, Processing, Injecting }
pub enum EngineEvent {
    State(EngineState),
    Error(String),
    Injected { words: u32, total_ms: u64 },
    DictionaryLearned(String),
    AppInsert { text: String },
}
pub enum EngineCmd { ApplySettings(Box<Settings>), Shutdown }

pub struct DictEntry { pub id: i64, pub word: String, pub starred: bool, pub source: String, pub use_count: i64 }
pub struct HistoryEntry { /* mirrors §11 */ }
pub struct InsightsSummary { pub total_words: i64, pub avg_wpm: f64, pub top_apps: Vec<(String, i64)>, pub daily_words: Vec<(String, i64)> }

pub trait Store: Send + Sync {
    // dictionary CRUD + star toggle + bump_use_count(words: &[String])
    // history: append(entry), list(limit, offset) -> Vec<HistoryEntry>
    // insights_summary() -> InsightsSummary
}
```

`vf-core` also exports the four default prompt consts (§9) and `default_settings()`. No I/O, no async deps in vf-core.

Engine entry point (implemented in vf-engine): `vf_engine::spawn(settings: Settings, store: Arc<dyn Store>) -> EngineHandle` where `EngineHandle { cmd_tx, events /* broadcast receiver factory */ }`.

## 13. Tauri IPC commands (app/src-tauri — thin wrappers only)

`get_settings`, `save_settings(settings)` (persist via vf-store, then `EngineCmd::ApplySettings`), `list_groq_models()` (vf-cloud), `list_input_devices()` (cpal names + "System default" pseudo-entry), `dictionary_list/add/update/delete/toggle_star`, `history_list(limit, offset)`, `history_delete` / `history_clear` (allowed — PRODUCT.md), `insights_summary()`, `reset_prompt(name)` → returns the vf-core default text, `set_autostart(enabled)` + `autostart_status()` (HKCU `Software\Microsoft\Windows\CurrentVersion\Run`, value `VillFlow` = exe path, via `winreg`).

Shell → frontend events: `engine-state`, `engine-error`, `engine-injected`, `app-insert` (when dictating while Settings is focused).

## 14. UI windows (app/ui)

**Main window "VillFlow"** — default tab **Overview** (PRODUCT.md happy path). On first run / until Ready: **always show** main window (ignore `start_minimized` until Ready after a successful save). Left nav →
- **Overview (default):** Ready checklist; How to use (live hotkey strings); status; **Save & apply**.
- Dictation: microphone, Auto Cleanup (none/light/medium/high); advanced: include field context toggle (**off** by default).
- Hotkeys: two shortcut-capture fields (dictation, command mode).
- Dictionary: table (word, starred ★, source, use_count) with add/edit/delete/star; auto-learn toggle (**default off**).
- Cloud & keys: full ElevenLabs key list (add/remove/reorder; masked), endpoint; Groq key, model picker (`list_groq_models`, refresh); key vault.
- Prompts: editors for light/medium/high/command/command_generate each with "Reset to default".
- Output: injection method radio (Clipboard paste / Simulated typing), restore-clipboard toggle.
- History: recent transcripts with Copy, **per-row delete**, and **Clear all**.
- Insights: total words, average WPM, top 5 apps, streak heatmap.
- App: launch at startup, start minimized (honored only after Ready), show error notifications, history retention.
- About: VillFlow, version, "V3", links to ElevenLabs and Groq docs.

**Ready checklist:** ≥1 ElevenLabs key; Groq key **or** cleanup `none`; mic OK; two valid distinct hotkeys each with a modifier.

**Not ready + hotkey:** engine refuses immediately with clear message (see §5).

**Tray:** V3 icon; tooltip reflects engine state / Needs setup; menu = Open VillFlow / Quit. Closing the main window hides to tray (app keeps running).

## 15. Dictionary auto-learn (vf-engine, best-effort)

Default **`dictionary.auto_learn` = false** (PRODUCT.md). When enabled: after injection, remember the injected text and UIA element; after ~8s re-read the element's text. Word-align injected vs. current; single-token replacements with edit distance 1–3, token length ≥ 4, not a common stopword → `dictionary.add(word, source='auto')`, max 3 per utterance. Silent no-op if the element can't be re-read. Also `bump_use_count` for dictionary words that appeared in the final text.

## 16. Process rules (all agents)

1. Read this file fully before coding. Touch only paths you own (§4).
2. Before ending a session: `cargo build --workspace` must pass (and `cargo clippy --workspace` should be clean; justify any remaining warnings in your summary). Commit everything: `git add -A && git commit -m "<agent>: <phase>: <summary>"`.
3. No dummy/placeholder implementations without a `// TODO(vf): <reason>` comment plus a line in your final summary.
4. Never add: telemetry, accounts, network calls beyond §6/§7, features listed in §2.
5. Do not modify `prompts/` or the two original human spec docs under `docs/internal/` unless the product owner asks. **CONTRACTS.md** and **docs/PRODUCT.md** may be updated when locking product decisions or aligning technical contracts. Prefer updating PRODUCT.md for behavior; keep this file in sync.
6. Never print or log API keys.
7. Sole maintainers may ignore multi-agent crate ownership walls when fixing cross-cutting bugs (e.g. mic + STT + UI).

## 17. Build order

| Phase | Agent | Scope |
|---|---|---|
| P0 | orchestrator (done) | repo, contracts, briefs, icon |
| P1 | Antigravity — session 1 | workspace scaffold, vf-core, vf-store, app shell stub |
| V1 | opencode | verify P1 |
| P2 | GrokBuild — session 1 | vf-cloud |
| V2 | opencode | verify P2 |
| P3 | GrokBuild — session 2 | vf-engine |
| V3 | opencode | verify P3 |
| P4 | Antigravity — session 2 | full UI vs real store/cloud |
| V4 | opencode | verify P4 |
| P5 | Antigravity — session 3 | integration: engine host, tray, autostart, notifications |
| V5 | opencode | verify P5 + release build |

## 18. Locked product defaults (see docs/PRODUCT.md — do not silently reverse)

- Command Mode: **dual** (Edit with selection / Generate without); selection lost at inject → insert at cursor + warning.
- Cleanup `none` bypasses the LLM. Default cleanup **medium**. `temperature 0.2` / `max_completion_tokens 8192`.
- Field context to LLM: **off by default**; advanced toggle only.
- Auto-learn default **OFF**, cap 3 words/utterance when on.
- Overlay bottom-center; labels Connecting / Recording / Processing / Edit / Generate as applicable. Engine-state matches: `Connecting` while STT opens, then `Recording` (or Edit/Generate via overlay label).
- History: list + copy + **delete row** + **clear all**.
- Keyterms: starred first, then use_count.
- Release: **portable exe + installer**.
- Setup-first UI; Save & apply on Setup; show window until Ready.
- "Communication profile" insight dropped (accepted limitation).
