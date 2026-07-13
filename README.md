# VillFlow

Windows push-to-talk voice dictation (Rust + Tauri v2). Hold `Ctrl+Shift+Z` anywhere, speak, release — polished text lands at your cursor. `Ctrl+Shift+X` transforms selected text by spoken command. `Ctrl+Shift+C` toggles the Scratchpad.

Authoritative spec: [CONTRACTS.md](CONTRACTS.md).

## Build & run

**Production build** (embeds the UI — the only correct way to build the exe):

```powershell
cd app
ui\node_modules\.bin\tauri.cmd build --no-bundle
# → target\release\villflow.exe
```

**Dev mode** (hot-reload UI; starts Vite automatically):

```powershell
cd app
ui\node_modules\.bin\tauri.cmd dev
```

> ⚠ Plain `cargo build` / `cargo run` produces a binary that tries to load the Vite dev server (`localhost:5173`) and shows *"localhost refused to connect"* if Vite isn't running. Always build the exe via the Tauri CLI as above. First-time setup: `cd app\ui && npm install`.

## Configuration

- Settings file: `%APPDATA%\VillFlow\settings.json` (auto-created with defaults on first run; editable in-app under Settings).
- Database (dictionary / history / scratchpad): `%APPDATA%\VillFlow\villflow.db`.
- API keys: ElevenLabs key list under **AI Services → ElevenLabs** (ordered; automatic failover top-to-bottom), Groq key + model picker under **AI Services → Groq**.

## Smoke test (validates keys + cloud clients, no microphone needed)

```powershell
cargo run --release -p vf-cloud --example live_smoke -- path\to\16khz_mono_s16le.wav
# without a wav argument, only the Groq checks run
```

## Tests

```powershell
cargo test --workspace
```
