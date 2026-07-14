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

## License

By contributing, you agree that your contributions are licensed under the MIT License (see [LICENSE](LICENSE)).
