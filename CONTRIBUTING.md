# Contributing to VillFlow

Thanks for your interest in contributing.

## Prerequisites

- Windows 11 (x86_64)
- [Rust stable](https://rustup.rs/) with the `x86_64-pc-windows-msvc` toolchain
- [Node.js 20+](https://nodejs.org/) (for the UI / Tauri CLI)
- Microsoft C++ Build Tools / Visual Studio with the “Desktop development with C++” workload

## Setup

```powershell
git clone https://github.com/<you>/VillFlow.git
cd VillFlow
cd app\ui
npm install
cd ..\..
```

## Development

```powershell
# Hot-reload UI + app
cd app
ui\node_modules\.bin\tauri.cmd dev
```

## Checks before opening a PR

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cd app\ui
npm run build
```

Production binary (embeds the UI):

```powershell
cd app
ui\node_modules\.bin\tauri.cmd build --no-bundle
# → target\release\villflow.exe
```

> Plain `cargo build` alone does **not** embed the frontend. Always use the Tauri CLI for a runnable release binary.

## Project layout

| Path | Role |
| ---- | ---- |
| `crates/vf-core` | Shared types, settings, defaults |
| `crates/vf-store` | `settings.json` + SQLite |
| `crates/vf-cloud` | ElevenLabs STT + Groq LLM |
| `crates/vf-engine` | Hotkeys, audio, inject, overlay, orchestrator |
| `app/src-tauri` | Tauri shell, tray, IPC |
| `app/ui` | Settings / Scratchpad UI (vanilla TS + Vite) |
| `CONTRACTS.md` | Product and architecture contracts |

Agent brief files under `prompts/` and root `AGENTS.md` / `CLAUDE.md` are optional notes for multi-agent workflows. Human contributors can ignore them.

## Guidelines

- Do not log or commit API keys
- Do not add telemetry, accounts, or network calls beyond ElevenLabs STT and Groq LLM
- Prefer small, focused pull requests with a clear description
- Match existing code style (Rust 2021, no extra UI frameworks)
- Product behavior: [docs/PRODUCT.md](docs/PRODUCT.md). Fix tracker: [docs/ISSUES-AND-FIX-PLAN.md](docs/ISSUES-AND-FIX-PLAN.md). Technical contracts: [CONTRACTS.md](CONTRACTS.md) (PRODUCT wins on behavior).

## Manual regression checklist

Run against a release build (`tauri build`) with real API keys when possible:

```text
Setup / first run
[ ] Fresh install or empty keys: main window opens on Setup, Ready = Needs setup
[ ] Add ElevenLabs + Groq keys → Save & apply → Ready green
[ ] Hotkey while Needs setup → toast “Add your API keys…”, no long Recording state

Dictation
[ ] Notepad: hold Ctrl+Shift+Z, speak from first word → full phrase appears
[ ] Overlay: Connecting… then Recording (with level), then Processing
[ ] Cleanup None is faster; Medium still inserts cleaned text
[ ] Speak into a browser text field and VS Code / similar

Command mode
[ ] Select text → hold Ctrl+Shift+X → Edit overlay → selection rewritten
[ ] No selection → Generate overlay → new text at cursor
[ ] Clear selection mid-hold after Edit started → toast about generate / insert

Scratchpad & tray
[ ] Ctrl+Shift+C toggles Scratchpad; dictate into it
[ ] Close main window → app stays in tray; Quit only from tray menu

Settings
[ ] Change hotkey, Save & apply, re-test
[ ] History: copy, delete row, clear all
[ ] Start minimized only after Ready (first run always shows window)

Cleanup
[ ] Quit from tray: no stuck Ctrl/Shift in other apps
```

## License

By contributing, you agree that your contributions are licensed under the MIT License (see [LICENSE](LICENSE)).
