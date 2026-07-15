# Changelog

All notable changes to VillFlow are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Redesign & vault (Phase 5)

- **UI redesign**: teal accent system, refined sidebar/cards, clearer hierarchy
- **Scratchpad rewrite**: plain-text `textarea` (reliable dictation), Markdown toolbar helpers
- **Portable vault**: passphrase AES-GCM sealed keys (cross-PC) in addition to default Windows DPAPI
- GitHub badges / links use `SreekarGpalli/VillflowV3`

### Final polish (Phase 4)

- README / clone / badges point at `SreekarGpalli/VillflowV3`
- GitHub Release workflow attaches portable exe + NSIS + MSI
- Scratchpad: numbered lists, undo, clear
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
- **Scratchpad** (`Ctrl+Shift+C`): floating always-on-top notes with rich-text toolbar and autosave
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
