# VillFlow — Product decisions (locked)

**Status:** locked by product owner (interactive decision session)  
**Date:** 2026-07-15  
**Precedence for product behavior:** this file > [ISSUES-AND-FIX-PLAN.md](ISSUES-AND-FIX-PLAN.md) > [CONTRACTS.md](../CONTRACTS.md) when they disagree.  
**Technical wire details** (APIs, schemas) still live in CONTRACTS unless this file overrides behavior.

Implementers and agents: if CONTRACTS conflicts with this file, **follow PRODUCT.md** and update CONTRACTS when touching that area.

---

## 1. What VillFlow is

| Attribute | Decision |
|-----------|----------|
| Model | Open source, free, not sold |
| Accounts | None |
| Distribution | GitHub: **portable `villflow.exe` + installer** (both) |
| Setup | User pastes own ElevenLabs + Groq API keys on their PC |
| Privacy | Local settings/DB/logs; no telemetry; no cloud accounts. API keys encrypted at rest with Windows DPAPI (current user). |
| Primary goal | Download → Setup → keys → hold hotkey → speak → text at cursor |

**v1 happy path:** Fresh machine → install or run exe → Setup → Ready → dictate into Notepad.

---

## 2. Command mode (locked)

**Hotkey default:** Ctrl+Shift+X (user-configurable).

**Dual mode:**

| Situation | Behavior | History `mode` | Overlay label |
|-----------|----------|----------------|---------------|
| Selection detected | Transform selection with spoken instruction | `command` | **Edit** |
| No selection | Generate new content at cursor | `command_generate` | **Generate** |

**Selection detection:** UIA first, then Ctrl+C clipboard fallback with save/restore (current approach).

**Selection lost after Edit started (gone at inject time):** Still insert result at cursor + short warning toast (e.g. “Selection lost — inserting at cursor”). Do not abort.

**Not ready / missing keys:** See §6 — refuse immediately; do not fake Recording.

---

## 3. Dictation & LLM (locked)

| Topic | Decision |
|-------|----------|
| Default cleanup level | **Medium** (needs Groq) |
| Field / document context to Groq | **Off by default** for new installs |
| Advanced: field context | **Yes, toggle in Advanced only** (not on main Setup card). When off, do not send field context (empty / omit). When on, send nearby text; prefer light safeguards, avoid mandatory double-Groq. |
| Latency claim | **Best effort only — no hard ms SLA** in docs. Still remove artificial sleeps in code. Typical cloud path may be ~0.5–2s; cleanup `none` is fastest. |
| Language | English only (Indian English accents) — unchanged |

---

## 4. Dictionary (locked)

| Topic | Decision |
|-------|----------|
| Auto-learn default | **Off** for new installs |
| Manual dictionary | Keep (add/star/edit/delete) |
| Auto-learn when enabled | Existing best-effort behavior (~8s re-read) is fine |

---

## 5. History (locked)

| Topic | Decision |
|-------|----------|
| Delete per row | **Yes** |
| Clear all | **Yes** |
| Old contract “no delete” | **Superseded** — update CONTRACTS |

---

## 6. Setup, UI, UX (locked)

### Navigation

- Default tab: **Setup** (not General).
- Setup holds: Ready checklist, How to use, API keys essentials, mic, hotkeys (summary or full), cleanup level, **Save & apply**.
- Advanced elsewhere: Prompts, Output details, full Cloud Keys (endpoint/multi-key), Dictionary, History, Insights, General (startup/tray), About.

### Ready checklist

Ready when:

- ≥1 ElevenLabs key, and  
- Groq key present **or** cleanup level is `none`, and  
- Mic configuration valid (system default OK), and  
- Three hotkeys valid (each has a modifier, all distinct).

### Save model

- Setup uses explicit **Save & apply** (persist + `ApplySettings` to engine).
- Not auto-save for Setup essentials.

### First run / window

- **Always show main window on Setup until Ready** (ignore `start_minimized` until user has been Ready after a successful save, or equivalent onboarding complete flag).
- After that, honor `start_minimized`.
- Close window = hide to tray; Quit only from tray (unchanged).

### Not ready + hotkey

- **Refuse immediately** with a clear message (overlay/toast): e.g. “Add your API keys in Setup.”
- Do **not** enter Recording / open STT.
- Prefer not to hard-steal focus from the target app every time (message is enough; optional gentle show of main window later if needed).

### Overlay states (happy path)

| State | Label |
|-------|--------|
| Pre-capture / STT connecting | **Connecting…** (when needed) |
| Capturing audio | **Recording** (only when mic is actually capturing) |
| After release | **Processing** |
| Command + selection | **Edit** |
| Command + no selection | **Generate** |
| Soft errors | Short toast on overlay |

### Phase 0 UI package (approved)

U1–U9 full package: Setup, nav, keys, Save & apply, status/last error, first-run window, hotkeys, how-to, overlay Connecting + Edit/Generate.

---

## 7. Engine Phase 0 (approved)

| ID | Decision |
|----|----------|
| A1 | Fix: mic from key-down; STT open in parallel |
| A3 | Fix: release/build path; ship portable **and** installer |
| A4 | Fix: remove self-inflicted latency (long settles, blocking toast sleeps, avoid needless double Groq when context off) |
| A5 | Fix: sticky last-good ElevenLabs key |

---

## 8. Distribution (locked)

- **Primary:** portable `villflow.exe` on GitHub Releases.  
- **Also:** Windows installer (NSIS and/or MSI via Tauri bundle) for users who want it.  
- Document both in README / PUBLISHING.  
- Production UI must be Tauri-embedded builds (not plain `cargo build` for end users).

---

## 9. Documentation (locked)

| Artifact | Role |
|----------|------|
| **docs/PRODUCT.md** (this file) | Product behavior decisions |
| **docs/ISSUES-AND-FIX-PLAN.md** | Issue tracker + how to fix; mark decisions locked |
| **CONTRACTS.md** | Technical contracts; **must be updated** to match this file where they diverge |
| **README.md** | Human onboarding aligned with Setup happy path |

---

## 10. Explicit non-goals (unchanged spirit)

No accounts, no telemetry, no paid tiers, no Wispr full parity, no multi-language v1, no streaming partials into target field (final paste once). Brand redesign not required for v1.

---

## 11. Decision log (session)

| # | Topic | Choice |
|---|--------|--------|
| 1 | Command mode | Dual: Edit if selection, Generate if not |
| 2 | Selection lost at inject | Insert at cursor + warning |
| 3 | Field context default | Off |
| 4 | Field context advanced | Toggle in Advanced only |
| 5 | Auto-learn default | Off |
| 6 | Setup save | Explicit Save & apply |
| 7 | First-run window | Show until Ready; then honor start_minimized |
| 8 | Latency docs | Best effort, no hard ms number |
| 9 | Distribution | Portable exe + installer |
| 10 | History delete | Keep delete + clear all |
| 11 | Phase 0 engine | A1, A3, A4, A5 all fix |
| 12 | Phase 0 UI | Full U1–U9 package |
| 13 | Hotkey while not ready | Refuse immediately, clear message |
| 14 | Selection detection | UIA then clipboard fallback |
| 15 | Overlay Generate/Edit | Always show mode labels |
| 16 | Product docs | PRODUCT.md + update CONTRACTS |
| 17 | Default cleanup | Medium |

---

*When changing product behavior later, edit this file first, then code, then CONTRACTS/README.*
