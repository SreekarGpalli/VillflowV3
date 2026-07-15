# VillFlow

**Windows push-to-talk voice dictation.** Hold a hotkey, speak, release — polished text lands at your cursor in any app.

[![CI](https://github.com/SreekarGpalli/VillflowV3/actions/workflows/ci.yml/badge.svg)](https://github.com/SreekarGpalli/VillflowV3/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%2011-0078D6)](#requirements)

---

## Features

| Hotkey | Action |
| ------ | ------ |
| **Ctrl+Shift+Z** | Dictation — hold to speak, release to paste cleaned text |
| **Ctrl+Shift+X** | Command mode — **Edit** if text is selected (rewrite it); **Generate** if nothing is selected (insert new text at cursor). Overlay shows which mode. |
| **Ctrl+Shift+C** | Toggle floating Scratchpad |

Latency is **best effort** (cloud APIs vary; cleanup **None** is fastest). See [docs/PRODUCT.md](docs/PRODUCT.md).

- **Tray-resident** — runs in the background; close the window to hide, not quit
- **ElevenLabs** realtime speech-to-text with ordered API-key failover
- **Groq** cleanup levels: none / light / medium / high
- **Dictionary** with preferred spellings, starring, and optional auto-learn
- **History & insights** — transcripts, WPM, top apps, activity heatmap
- **Privacy-first** — keys and data stay on your PC; no accounts, no telemetry

## Requirements

- **Windows 11** (x64)
- An [ElevenLabs](https://elevenlabs.io/) API key (speech-to-text)
- A [Groq](https://console.groq.com/) API key (text cleanup / commands)
- Microphone access

## Download

See [Releases](https://github.com/SreekarGpalli/VillflowV3/releases) for prebuilt `villflow.exe` (when published).

Or build from source below.

## Quick start (from source)

### 1. Install tools

- [Rust](https://rustup.rs/) (stable, MSVC toolchain)
- [Node.js 20+](https://nodejs.org/)
- Visual Studio **Desktop development with C++** (or Build Tools)

### 2. Clone and install UI deps

```powershell
git clone https://github.com/SreekarGpalli/VillflowV3.git
cd VillflowV3
cd app\ui
npm install
cd ..\..
```

### 3. Run in development

```powershell
cd app
ui\node_modules\.bin\tauri.cmd dev
```

### 4. Production build

**Portable exe** (no installer):

```powershell
cd app
ui\node_modules\.bin\tauri.cmd build --no-bundle
# → target\release\villflow.exe
```

**Portable + installer** (NSIS/MSI under `target\release\bundle\`):

```powershell
cd app
ui\node_modules\.bin\tauri.cmd build
```

GitHub Releases should attach **both** `villflow.exe` and the installer when possible.

> **UI embed:** The app crate defaults to Tauri’s `custom-protocol` feature, so `cargo build --release -p villflow` embeds `app/ui/dist` (run `npm run build` in `app/ui` first, or use the Tauri CLI which runs that automatically). Prefer `tauri build` for installers and a one-command release.

### First-run

1. Open VillFlow (Setup tab is first).
2. Add ElevenLabs + Groq keys → **Save & apply**.
3. When Ready is green, hold **Ctrl+Shift+Z** in Notepad and speak.

### 5. Configure keys

1. Start VillFlow (tray icon)
2. Open **VillFlow** from the tray
3. **AI Services** → add your ElevenLabs key(s) and Groq key
4. Optionally pick a Groq model (Refresh loads the live list)

Settings live at `%APPDATA%\VillFlow\settings.json`.

## Configuration paths

| Path | Purpose |
| ---- | ------- |
| `%APPDATA%\VillFlow\settings.json` | Settings & API keys |
| `%APPDATA%\VillFlow\villflow.db` | Dictionary, history, scratchpad |
| `%APPDATA%\VillFlow\logs\villflow.log` | Application log |

## Architecture

```
crates/vf-core     Shared types & settings defaults
crates/vf-store    settings.json + SQLite
crates/vf-cloud    ElevenLabs STT + Groq LLM
crates/vf-engine   Hotkeys, audio, UIA, inject, overlay, orchestrator
app/src-tauri      Tauri shell, tray, IPC
app/ui             Settings UI + Scratchpad (vanilla TS + Vite)
```

Product contracts and design decisions: [CONTRACTS.md](CONTRACTS.md).

## Tests

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Optional cloud smoke test (uses keys from your settings file):

```powershell
cargo run --release -p vf-cloud --example live_smoke
```

## Privacy & security

- API keys are stored only in local `settings.json` and are never logged
- Network: ElevenLabs STT + Groq LLM only
- No analytics, no account system, no auto-update

See [SECURITY.md](SECURITY.md) for vulnerability reporting.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## License

[MIT](LICENSE) © VillFlow contributors
