# Changelog

All notable changes to VillFlow are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.2] — 2026-08-02

### Fixed
- **Long dictation cut off at the end**: wait up to 5s for the mic feed to fully drain into STT before commit (was a hard 80ms timeout that dropped trailing speech on bigger holds)
- **STT backpressure**: deepen the realtime command queue (64 → 512) so long utterances are less likely to stall the feed path
- **Feed task lock**: stream audio through a cloneable feed handle so the session mutex is not held across network I/O
- **Multi-segment STT commits**: concatenate premature/server commits instead of keeping only the first segment
- **Silent mic frame drops**: deeper capture queue and warn in the log when frames are dropped
- **Groq gpt-oss truncation**: use low reasoning effort for cleanup so the token budget goes to the answer; on `finish_reason=length`, fall back to the raw STT transcript
- **Cleanup looks cut**: if cleaned text is much shorter than raw and ends unfinished, paste raw STT instead

### Added
- Per-utterance log line with raw/final character and word counts plus `final_vs_raw=%` for easier diagnosis

## [0.2.1] — 2026-07-16

### Fixed
- **Groq free-tier 413 TPM**: completion budget presets are now 1024 / 2048 / 4096 (default **2048**); old 8192 values snap down so cleanup no longer fails with “payload too large”
- **Dictation still works when Groq fails**: retry at 1024 tokens on 413, then paste **raw STT** with a toast instead of inserting nothing
- **Command mode 413**: one retry at 1024 max tokens
- **Connecting vs Recording mismatch**: Setup pill and overlay both show Connecting until STT is ready, then Recording
- **Command Edit selection**: keep key-down selection if mid-hold re-read is empty (do not demote to Generate)
- **Not-ready errors**: surface the real readiness message (ElevenLabs vs Groq) on engine-error

### Added
- **Max cleanup length** setting under Cloud & keys (preset token budgets for Groq)

## [0.2.0] — 2026-07-15

### Removed
- **Scratchpad** feature entirely (window, hotkey, tray item, store API, UI). App is dictation-focused only.

### Redesign & vault (Phase 5)

- **UI redesign**: Material-style dark theme, refined sidebar/cards, clearer hierarchy
- **Portable vault**: passphrase AES-GCM sealed keys (cross-PC) in addition to default Windows DPAPI
- GitHub badges / links use `SreekarGpalli/VillflowV3`

### Final polish (Phase 4)

- README / clone / badges point at `SreekarGpalli/VillflowV3`
- GitHub Release workflow attaches portable exe + NSIS + MSI
- Dictionary export JSON; About page privacy/license/GitHub links
- Product plan: all planned phases marked complete

### Improved (Phase 3)

- **DPAPI at rest**: ElevenLabs and Groq keys encrypted in `settings.json` for the current Windows user (legacy plaintext migrated on save)
- **History export**: Export JSON from the History tab
- **Test microphone**: Setup samples peak level with a simple meter
- **Show/hide Groq key** on Setup
- **PR template** and SECURITY notes for DPAPI

### Improved (Phase 2)

- **WPM / Insights**: store `speech_ms` (key-down → key-up) and use it for average WPM when available
- **Multi-monitor overlay**: Flow Bar sits on the monitor of the focused window
- **Partial STT preview**: live transcript snippet on the overlay while holding
- **History retention**: General setting — forever / 30 / 90 / 365 days (purge on startup and after dictation)
- **A11y**: nav roles, keyboard activation for tabs, status live region
- **Community**: GitHub bug/feature issue templates

### Improved (Phase 0–1 product pass)

- **Setup-first UI**: Ready checklist, How to use, keys, mic, cleanup, Save & apply; first-run always shows the window until Ready
- **Mic from key-down**: audio capture starts immediately while STT connects (less lost speech); overlay **Connecting…** / **Recording** / **Edit** / **Generate**
- **Not ready gate**: missing API keys refuse immediately with a clear toast
- **Sticky ElevenLabs key**: last working key preferred on the next utterance
- **Lower self-inflicted latency**: shorter clipboard/modifier waits; no blocking toast sleep before inject
- **Field context off by default** (advanced toggle under Dictation); auto-learn **off** by default
- **Command mode trust**: toast when selection is lost mid-command (falls back to generate); last dictation summary on Setup; pipeline timings in logs
- Docs: PRODUCT decisions, fix plan, CONTRIBUTING manual checklist; portable **and** installer release guidance

## [0.1.0] — 2026-07-13

First public release.

### Features

- **Push-to-talk dictation** (`Ctrl+Shift+Z`): hold, speak, release — cleaned text pastes at the cursor in any app
- **Command mode** (`Ctrl+Shift+X`): transform selected text, or generate new content when nothing is selected
- **ElevenLabs** realtime STT with ordered API-key failover
- **Groq** cleanup (none / light / medium / high) and command prompts
- System tray app (hide-to-tray, state tooltip, notifications)
- Dictionary with starring, use counts, and optional auto-learn
- History, insights (WPM, top apps, heatmap), and customizable prompts
- Clipboard-paste or simulated typing injection; clipboard restore option
- Native Win32 Flow Bar overlay (Recording / Processing)

### Privacy

- Settings and keys stay local under `%APPDATA%\VillFlow\`
- No accounts, no telemetry, no auto-update

### Platform

- Windows 11 (x86_64 MSVC)
- Rust + Tauri v2 + vanilla TypeScript / Vite
