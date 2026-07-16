# Antigravity brief — owns vf-core, vf-store, app/ (3 sessions)

Run one session block per phase, in order. Do not start a later session before the earlier one is verified.

---

## SESSION 1 (P1) — workspace scaffold + vf-core + vf-store

Read `CONTRACTS.md` (all of it). Then build:

1. Cargo workspace per §4: members `crates/vf-core`, `crates/vf-store`, `crates/vf-cloud`, `crates/vf-engine`, `app/src-tauri`. Create `vf-cloud` and `vf-engine` as EMPTY lib crates (bare `lib.rs`) — GrokBuild owns their contents; do not define anything in them.
2. `vf-core` per §12: settings structs mirroring §10 exactly (serde, defaults), enums, events, `Store` trait, `DictEntry`/`HistoryEntry`/`InsightsSummary`, the four default prompt consts verbatim from §9, `default_settings()`. Types only — no I/O.
3. `vf-store` per §10–§11: settings.json load/save with missing-field defaulting and atomic write; rusqlite (bundled) store implementing `Store`, DDL exactly as §11; insights queries as §11 describes. Unit tests for settings round-trip and insights math (use a temp dir + in-memory DB).
4. `app/`: Tauri v2 scaffold, vanilla TS + Vite, identifier `com.villflow.app`, product name VillFlow. Run `cargo tauri icon assets/icon.png`. Main window opens with an empty left-nav shell (§14 section names as disabled stubs). No engine, no IPC beyond a `get_settings` proof-of-life command.
5. `cargo build --workspace` green, tests green, then commit: `antigravity: P1: scaffold + vf-core + vf-store`.

Constraints: §16 rules; §3 dependency whitelist; do not implement UI sections, engine, or cloud code in this session.

---

## SESSION 2 (P4) — full UI

Prereq: P1–P3 committed (vf-cloud and vf-engine exist). Read `CONTRACTS.md` §13–§14 again before starting.

1. Implement every IPC command in §13 in `app/src-tauri`, wired to the real `vf-store` `Store` impl and `vf-cloud::list_groq_models` / cpal device enumeration. Engine-dependent behavior (`save_settings` → `ApplySettings` push, engine events) may use a `// TODO(vf): wired in P5` stub ONLY at the engine boundary.
2. Build the full main window UI per §14: General, Dictation, Hotkeys, Dictionary, AI Services, Prompts, Output, History, Insights, About. Hotkey capture fields record modifier+key combos as strings like `Ctrl+Shift+Z`. ElevenLabs keys: ordered list, masked, add/remove/reorder. Model picker fetches live with refresh; default `openai/gpt-oss-120b` preselected when the list can't be fetched.
3. Dark minimal theme, system font stack, no UI libraries. Every settings control round-trips through `get_settings`/`save_settings` (no UI-only state). (No Scratchpad window — product is dictation-only.)
4. Frontend builds (`npm run build` inside app/ui), `cargo build --workspace` green, commit: `antigravity: P4: full UI`.

Do not touch `crates/vf-cloud` or `crates/vf-engine`.

---

## SESSION 3 (P5) — integration

Prereq: P4 verified. Read `CONTRACTS.md` §5, §12, §13.

1. On app startup: load settings via vf-store → `vf_engine::spawn(settings, store)` → hold `EngineHandle` in Tauri state.
2. Bridge engine events → tray tooltip/state, `engine-state` + `engine-error` frontend events, Windows notification on error when enabled.
3. `save_settings` now pushes `EngineCmd::ApplySettings`. Implement `set_autostart`/`autostart_status` per §13. Implement `start_minimized` (launch hidden to tray) and close-to-tray per §14.
4. Verify the full loop compiles and the app runs: tray appears, main window opens, engine reaches Idle.
5. `cargo build --workspace` green, commit: `antigravity: P5: integration`.
