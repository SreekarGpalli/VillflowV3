# Changelog

All notable changes to VillFlow are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
