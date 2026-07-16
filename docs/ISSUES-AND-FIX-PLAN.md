# VillFlow — Issues, Decisions & Fix Plan

**Status:** living document for the maintainer  
**Last updated:** 2026-07-15  
**Audience:** you (product owner) + anyone fixing issues  
**Related:** **[PRODUCT.md](PRODUCT.md) (locked decisions)**, [CONTRACTS.md](../CONTRACTS.md), [README.md](../README.md), [docs/internal/VERIFY-REPORT.md](internal/VERIFY-REPORT.md), [docs/PUBLISHING.md](PUBLISHING.md)

**Contents:** locked decisions → product context → checklist → issue register → **§5 Happy path UI** → roadmap → patterns → code map.

> **Decisions are locked** (2026-07-15 session). Authoritative product file: **[docs/PRODUCT.md](PRODUCT.md)**. Do not re-litigate defaults while implementing — change PRODUCT.md first if the owner revises.

---

## 0. Locked decisions (summary)

| Topic | Locked choice | Implement |
|-------|---------------|-----------|
| Command mode | Dual: Edit + Generate | **done** Overlay Edit/Generate |
| Selection lost at inject | Insert + warning | **done** (no delay) |
| Field context | Off by default; Advanced toggle | **done** |
| Auto-learn default | **Off** | **done** |
| Setup save | Explicit **Save & apply** | **done** |
| First-run window | Show until **Ready** | **done** |
| Latency docs | Best effort, no hard ms | **done** (code sleeps reduced) |
| Distribution | Portable **+** installer | **done** (docs) |
| History | Delete row + Clear all | **done** (kept) |
| Phase 0 engine | A1, A3, A4, A5 | **done** |
| Phase 0 UI | Full U1–U9 | **done** |
| Not ready + hotkey | Refuse immediately | **done** |
| Selection detect | UIA then clipboard | **done** (kept) |
| Default cleanup | Medium | **done** (kept) |
| Product docs | PRODUCT.md + CONTRACTS aligned | **done** |

Full log: [PRODUCT.md §11](PRODUCT.md).

---

## 1. Product context (source of truth for priorities)

VillFlow is:

| Attribute | Decision |
|-----------|----------|
| Distribution | Open source, free, download from GitHub |
| Business model | None — not priced, not sold |
| Accounts | None — no sign-in |
| Setup model | User puts their own API keys (ElevenLabs STT + Groq LLM) on their PC |
| Runtime | Local Windows app; keys and data stay on the machine |
| Primary goal | Download → add keys → hold hotkey → speak → text appears in the focused app |

**Implications:**

- Optimize for **download → keys → it works**, not commercial onboarding or brand polish.
- Prefer a **portable `villflow.exe`** on GitHub Releases.
- Treat [CONTRACTS.md](../CONTRACTS.md) as a **technical draft written by AI**. When it conflicts with this document or with real product choices, **update the contracts** — do not “fix” code to a bad or stale spec without a deliberate decision.
- Do **not** add accounts, telemetry, cloud sync of keys, or paid features.

**v1 exit criteria:**

> Fresh machine → download exe (or documented build) → paste ElevenLabs + Groq keys → dictate into Notepad reliably.

---

## 2. How to use this document

1. Read **§1** (context) so priorities stay aligned.
2. Use **§3 Decision checklist** to mark what you want (Fix now / Later / Skip).
3. For engine/reliability items: **§4**. For first-run, settings layout, save model, overlay, tray: **§5**.
4. Work **Phase 0 → Phase 1 → Phase 2** (§6). Phase 0 includes both engine **and** happy-path UI.
5. When you finish an item, set its **Status** to `done` and note the date/PR if useful.
6. After major product decisions (especially command mode), rewrite or trim CONTRACTS so agents and humans share one truth.

### Severity scale

| Level | Meaning |
|-------|---------|
| **Critical** | Core loop broken or lies to the user (e.g. “Recording” without mic) |
| **High** | First-run or release fails for a typical GitHub user |
| **Medium** | Wrong often enough to hurt trust; workaround exists |
| **Low** | Polish, edge case, or nice-to-have |
| **Product** | Not a bug until you choose intended behavior |
| **Process** | Affects maintainers/docs/agents, not end users directly |

### Recommendation scale

| Tag | Meaning |
|-----|---------|
| **Fix** | Should change for a usable OSS v1 |
| **Improve** | Better if fixed; not blocking if delayed |
| **Keep** | Acceptable as-is under OSS/free constraints |
| **Decide** | You must pick product behavior, then implement |
| **Drop** | Do not spend time on this for v1 |

### Effort scale

| Tag | Rough size |
|-----|------------|
| **S** | Hours |
| **M** | ~1–2 days |
| **L** | Multi-day / large redesign |

### Status (tracking)

Use: `open` | `in progress` | `done` | `wontfix` | `deferred`

---

## 3. Decision checklist (fill this in)

Copy mentally or edit this file:

```
Engine / ship                    STATUS
[x] A1 Mic parallel STT          → FIX (locked)
[x] A3 Release portable+installer → FIX (locked)
[x] A4 Self-inflicted latency    → FIX (locked)
[x] A5 Sticky STT key            → FIX (locked)

Happy path UI (§5)               STATUS
[x] U1–U9 full package           → FIX (locked)

Product decisions                LOCKED
[x] B1 Dual mode Edit|Generate
[x] B2 History delete + clear
[x] B3 Field context off; Advanced toggle
[x] B4 Best effort latency (no hard ms)
[x] C4 Auto-learn default OFF
[x] F1 PRODUCT.md + CONTRACTS updated
```

**Implementation status:** Phase 0–2 items in §6 are implemented; §4 statuses match code as of 2026-07-15. Remaining **accepted** items (C1, C3, C5) are inherent platform limits, not unfinished work.

---

## 4. Full issue register

### A — Must get right for “download and use”

#### A1. Mic starts only after STT WebSocket opens

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | Critical |
| **Recommendation** | Fix |
| **Effort** | M |
| **Where** | `crates/vf-engine/src/orchestrator.rs` (`begin_utterance`), `crates/vf-engine/src/audio.rs`, `crates/vf-cloud/src/stt.rs` |

**Problem:** On key-down the engine shows Recording, reads context, then awaits `SttSession::open` (WebSocket handshake). Microphone capture starts only after that. Speech at the start of the hold is never recorded. Short phrases can fail entirely.

**Why it matters (OSS):** This is the product. Users will not debug “I held the key but the first words vanished.”

**How to fix:**

1. On key-down: start capture + ring buffer **immediately**.
2. Open STT in parallel.
3. When the socket is ready (and ideally after `session_started`), flush buffered PCM, then stream live.
4. UI state: prefer `Connecting…` then `Recording`, or only show Recording once capture is running.
5. Keep existing mid-utterance STT buffer/resend for key rotation.

**Done when:** Holding the hotkey and speaking from the first moment captures the full phrase in Notepad.

---

#### A2. First launch with empty API keys

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | High |
| **Recommendation** | Fix — implement via **§5 (U1–U9)**, not a one-off toast |
| **Effort** | M (with full Setup tab) |
| **Where** | `app/ui/`, `app/src-tauri/src/main.rs` — full plan in **§5** |

**Problem:** Defaults write empty keys. Engine starts. First hotkey fails at STT open with an error. No first-run checklist; Cloud Keys is buried in nav; landing tab is “General” (startup checkboxes).

**Why it matters (OSS):** No accounts means keys *are* the only gate. That gate must be the **center of the UI**, not a buried tab.

**How to fix:** Do not only add a toast. Implement the **happy-path UI package** in **§5**: Setup tab, Ready checklist, keys + mic + hotkeys on one screen, save/apply essentials, status, first-run window, short “how to use” copy.

**Done when:** §5 Phase-0 UI exit criteria pass (see §5.9).

---

#### A3. Production build / GitHub release path

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | High |
| **Recommendation** | Fix |
| **Effort** | S (docs) / M (CI) |
| **Where** | [README.md](../README.md), [docs/PUBLISHING.md](PUBLISHING.md), `app/src-tauri/Cargo.toml`, `app/src-tauri/tauri.conf.json`, GitHub Actions |

**Problem (historical):** Without Tauri `custom-protocol`, release binaries loaded `devUrl` (`localhost:5173`) → blank WebView / connection refused unless Vite was running.

**Fix landed:** `app/src-tauri/Cargo.toml` defaults to `custom-protocol` so `cargo build --release -p villflow` embeds `frontendDist` (`app/ui/dist`). Prefer `tauri build` for installers + automatic UI rebuild. Portable exe + optional MSI/NSIS; CI attach to Releases.

**Done when:** A stranger can install from Releases (or follow one build command) and see the real UI without a Vite dev server.

---

#### A4. Self-inflicted post-release latency

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | High (feel) / Medium (if text always correct) |
| **Recommendation** | Fix delays you control; soften hard SLA in docs |
| **Effort** | S–M |
| **Where** | `crates/vf-engine/src/inject.rs` (`settle_modifiers`), `orchestrator.rs` (toasts, double Groq, selection re-read) |

**Problem:** After key-up, code can stack: feed drain timeout, STT commit, Groq, optional **second** Groq (suspicious output retry), selection re-read, **400ms toast sleep**, **`settle_modifiers` up to 800ms**, clipboard settle + paste + restore. AI contract claimed &lt;700ms total after release — that is not a realistic hard SLA and is contradicted by fixed sleeps.

**Why it matters (OSS):** You cannot control API RTT, but fixed multi-hundred-ms sleeps make the app feel broken.

**How to fix:**

1. Happy path: don’t wait long for modifiers already released; force-release only if still down after a short poll.
2. Don’t delay inject for toast readability on the happy path (toast async / non-blocking).
3. Avoid double Groq unless necessary; prefer simpler prompts/defaults (see B3).
4. Log `release → stt_done → llm_done → inject_done` timings for debugging.
5. Docs: “best effort; typical ~0.5–2s on a good network; cleanup `none` is fastest.”

**Done when:** Notepad dictation with cleanup `none` feels snappy; medium cleanup doesn’t sit idle on artificial sleeps.

---

#### A5. Dead first ElevenLabs key retried every utterance

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | Medium–High (multi-key users) |
| **Recommendation** | Fix |
| **Effort** | S |
| **Where** | `crates/vf-cloud/src/stt.rs` (`KeyRotator`), orchestrator session open |

**Problem:** Each utterance builds a rotator starting at index 0. A dead first key costs handshake failure every time.

**How to fix:** Remember last successful key index in process memory (optional: persist in settings). Start next utterance there; keep one-cycle rotation on failure.

**Done when:** After a successful rotation, the next utterance does not re-fail the bad key first.

---

### B — Product decisions (choose, then lock in docs)

#### B1. Command mode: transform vs generate

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | Product |
| **Recommendation** | Decide, then implement UX + docs |
| **Effort** | S–M |

**Current code:** No selection → generate new text (`command_generate`). With selection → transform (`command`).  
**CONTRACTS §5/§18:** No selection → toast “Select text first”, abort.  
**VERIFY-REPORT:** Documents intentional drift.

**Options:**

| Option | Behavior | Pros | Cons |
|--------|----------|------|------|
| **A** | Selection required | Simple, safer | Less powerful |
| **B** | Dual mode (current) | Powerful free-tool UX | Needs visible mode; selection false-negatives are dangerous |

**If A:** Remove generate path and `PROMPT_COMMAND_GENERATE`; toast + abort.  
**If B (recommended for OSS):** Overlay or status must show **Edit** vs **Generate**; document both in README; consider safer selection policy (C2).

**Done when:** Behavior matches your choice and is obvious to the user + documented.

---

#### B2. History delete / clear

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | Low (feature is good) |
| **Recommendation** | **Keep** feature; update contracts |
| **Effort** | S |

**Problem:** CONTRACTS said history is list + copy only. Code/UI has delete and clear — correct for privacy on a local speech app.

**How to fix:** Update product docs/contracts to allow delete/clear. Optional: stronger confirm copy before clear.

**Done when:** Docs match code; no agent tries to remove delete as a “bug.”

---

#### B3. Field context → LLM + rewrite safeguards

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | Medium |
| **Recommendation** | Improve (simplify) |
| **Effort** | M |
| **Where** | `crates/vf-core` prompts, `crates/vf-cloud/src/sanitize.rs`, `orchestrator.rs` |

**Problem:** Models rewrote documents when given field context. Codebase added hardened prompts, `strip_context_echo`, `dictation_output_suspicious`, and retry without context (extra latency/cost). High complexity for maintainers.

**How to fix (suggested):**

1. Default: cleanup **without** document field context (keep dictionary + app name).
2. Optional advanced setting: “Include nearby text for continuity.”
3. Reduce reliance on double Groq + heuristics.
4. Align default prompt strings with that policy; migrate settings carefully.

**Done when:** Default dictation rarely rewrites existing document text and rarely needs a second LLM call.

---

#### B4. Hard “&lt;700ms after release” claim

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | Doc honesty |
| **Recommendation** | Drop as hard requirement |
| **Effort** | S |

**How to fix:** Rewrite CONTRACTS/README to best-effort language. Measure in logs; do not market a number you don’t enforce in CI.

---

#### B5. Portable exe vs installers

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | Low |
| **Recommendation** | Portable exe primary; installers optional |
| **Effort** | S |

**How to fix:** Align `tauri.conf` / release notes / PUBLISHING with “primary artifact = portable exe.”

---

### C — Reliability / platform limits

#### C1. Text injection (clipboard / SendInput)

| Field | Value |
|-------|--------|
| **Status** | accepted |
| **Severity** | Medium (inherent) |
| **Recommendation** | Improve docs + defaults; accept limits |
| **Effort** | S docs / M per app bug |
| **Where** | `crates/vf-engine/src/inject.rs` |

**Problem:** Clipboard race, elevated windows, some apps ignore synthetic input. Not fully solvable.

**How to fix:** Document known-good apps; optional “don’t restore clipboard”; manual test matrix in CONTRIBUTING (Notepad, Chrome, VS Code, Word, Slack).

---

#### C2. Selection read for command-edit

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | Medium |
| **Recommendation** | Improve if dual mode (B1-B) |
| **Effort** | M |
| **Where** | `crates/vf-engine/src/context.rs`, orchestrator command path |

**Problem:** UIA often fails; Ctrl+C fallback is racy. False “no selection” → generate instead of edit can insert wrong content.

**How to fix:** Prefer UIA; clipboard only if UIA empty; if ambiguous, toast instead of generate; or separate hotkeys for Edit vs Generate.

---

#### C3. In-app insert (Settings WebView)

| Field | Value |
|-------|--------|
| **Status** | accepted |
| **Severity** | Low (Settings WebView only; primary path is external apps) |
| **Recommendation** | Keep AppInsert; improve focus routing if needed |
| **Effort** | S–M |
| **Where** | `inject.rs`, `app/src-tauri/src/main.rs` (`emit_app_insert`), UI listeners |

**Problem:** WebView2 ignores SendInput → AppInsert event. Focus heuristics can deliver to the wrong window.

**How to fix:** Single target: focused VillFlow window only; log target; never emit to both.

---

#### C4. Auto-learn dictionary

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | Low |
| **Recommendation** | Keep feature; prefer default **off** |
| **Effort** | S |
| **Where** | `crates/vf-engine/src/autolearn.rs`, settings UI |

**How to fix:** Clear UI copy; default `dictionary.auto_learn` to false unless you strongly want it on.

---

#### C5. Hotkeys (LL hook)

| Field | Value |
|-------|--------|
| **Status** | accepted |
| **Severity** | Low (mostly solid) |
| **Recommendation** | Keep; small docs/improvements only |
| **Effort** | S |
| **Where** | `crates/vf-engine/src/hotkeys.rs` |

**Notes:** Modifier not swallowed, require modifier, unique combos — good. Long-tail: AltGr, OS conflicts, install timeout still proceeding.

---

#### C6. Overlay (primary monitor, minimal)

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | Low–Medium |
| **Recommendation** | Keep for v1; improve later |
| **Effort** | M later |
| **Where** | `crates/vf-engine/src/overlay.rs` |

**Later:** Monitor with focus / near caret; optional partial transcript.

---

#### C7. Partial STT transcripts unused

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | Low |
| **Recommendation** | Keep unused for v1 |
| **Effort** | M if added |

`SttSession::subscribe_partials` exists; engine does not drive overlay with partials.

---

### D — UI / UX (summary; **full happy-path plan is §5**)

Issues D1–D2 and A2 are specified end-to-end in **§5 (U1–U9)**. Below is the short register only.

#### D1. Ten tabs / settings-first IA

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | High for first-run (not just “Medium polish”) |
| **Recommendation** | Fix as part of Phase 0 UI (§5 U1–U2) |
| **Effort** | M |
| **Where** | `app/ui/index.html`, `app/ui/src/main.ts` |

**How to fix:** See **§5.2–§5.3** — new **Setup** default tab; reorder nav; demote General/Prompts/etc.

---

#### D2. Explicit Save bar

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | Medium on happy path (keys/mic/hotkeys not applied) |
| **Recommendation** | Fix as **§5.5 (U4)** |
| **Effort** | S–M |

**How to fix:** Essentials (keys, mic, hotkeys, cleanup) save/apply clearly; optional auto-apply on Setup. See §5.5.

---

#### D3. Visual design / brand

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | Low |
| **Recommendation** | Google Material 3 dark theme applied (Settings) |

---

#### D4. Scratchpad (removed)

| Field | Value |
|-------|--------|
| **Status** | removed |
| **Severity** | — |
| **Recommendation** | **Removed** — product is dictation-only (no notes window) |
| **Effort** | — |

---

#### D5. Accessibility

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | Medium ethically / Low early OSS |
| **Recommendation** | Basic a11y: roles, labels, focusable nav, keyboard hotkey capture |
| **Effort** | M |

---

#### D6. Insights / WPM / heatmap

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | Low |
| **Recommendation** | Keep tab; not on happy path; fix WPM later |
| **Effort** | S for metric |

**Problem:** Duration includes STT open / processing, so WPM is misleading.

**How to fix later:** Speech-only interval (capture start → key-up) or exclude processing time.

---

### E — Privacy / security

#### E1. Plaintext API keys in `settings.json`

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | Accepted for model |
| **Recommendation** | DPAPI default + optional passphrase vault (AES-GCM) |
| **Effort** | S docs / M encrypt |

Path: `%APPDATA%\VillFlow\settings.json`. Never log keys (already a rule).

---

#### E2. Full transcript history retention

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | Medium (privacy) |
| **Recommendation** | Keep history + clear; optional auto-retention later |
| **Effort** | S–M |

---

#### E3. No telemetry / no accounts

| Field | Value |
|-------|--------|
| **Status** | done (by design) |
| **Recommendation** | Keep |

---

### F — Architecture / process

#### F1. CONTRACTS.md AI-written and drifted

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | High for maintainers |
| **Recommendation** | Fix after product decisions |
| **Effort** | M |

**How to fix:** Write a short product section (or `docs/PRODUCT.md`) you believe. Update CONTRACTS for: command mode, history delete, prompt schema, portable release, latency language, AppInsert. Point agents at the living doc.

---

#### F2. Multi-agent crate ownership walls

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | Process |
| **Recommendation** | Relax if you are sole maintainer |

Cross-cutting fixes (A1) should touch engine + cloud + UI as needed.

---

#### F3. No full e2e PTT in CI

| Field | Value |
|-------|--------|
| **Status** | done |
| **Severity** | Medium (maintenance) |
| **Recommendation** | Improve |
| **Effort** | S checklist / L full e2e |

**How to fix:** Manual checklist in CONTRIBUTING; keep `live_smoke` / headless engine for partial automation.

---

#### F4. Crate whitelist pedantry (`chrono`, etc.)

| Field | Value |
|-------|--------|
| **Status** | wontfix (unless you care) |
| **Severity** | None for users |
| **Recommendation** | Ignore for v1 |

---

### G — Explicitly deprioritize (do not block v1)

| Topic | Verdict |
|-------|---------|
| Commercial onboarding funnel | Drop — use Setup checklist (A2) only |
| Brand / marketing redesign | Drop for now |
| Multi-monitor overlay perfection | Later (C6) |
| Live partials on overlay | Later (C7) |
| Full a11y audit | Later (D5) |
| Hard &lt;700ms SLA | Drop (B4) |
| Wispr feature parity | Drop — keep non-goals |
| MSI/NSIS as required ship form | Optional only (B5) |
| Insights as core product | Bonus only (D6) |
| Perfect UIA in every app | Document limits (C1/C2) |

---

## 5. Happy path — Settings, UI & UX (detailed fix plan)

This section is the **settings + UI + UX** plan for the only journey that matters:

> Download → open VillFlow → paste keys → (optional) check mic/hotkeys → open Notepad → hold hotkey → speak → release → text appears.

Engine fixes (A1, A4, A5) make that path *work*. **This section makes the path *obvious and hard to mess up*.**

### 5.1 Target happy path (step by step)

| Step | User does | App must do |
|-----:|-----------|-------------|
| 1 | Runs `villflow.exe` from GitHub | Main window opens (first run: **not** hidden to tray only). UI loads from embedded assets (A3). |
| 2 | Sees **Setup** first | Not “General → Launch at startup”. Big **Ready** checklist. |
| 3 | Pastes ElevenLabs key(s) + Groq key | Fields on Setup; mask secrets; links to get keys; **Apply / Save** clearly applies to engine. |
| 4 | Sees Ready go green | Checklist: ElevenLabs ✓, Groq ✓ (if cleanup ≠ none), mic present, hotkeys valid. |
| 5 | Reads one short “How to use” block | Defaults: hold **Ctrl+Shift+Z** dictate, **Ctrl+Shift+X** command. |
| 6 | Opens Notepad, holds Z combo | Overlay: Connecting (if needed) → **Recording** (mic actually on). Level pulse optional. |
| 7 | Releases | Overlay **Processing** → text pastes → Idle. Tray tooltip matches. Errors: toast + optional Windows notification + **last error on Setup**. |
| 8 | Later: tweaks | Advanced tabs (Prompts, Output, Dictionary, History, Insights, General) stay available but not on the critical path. |

**Not on the happy path (do not force users through these to dictate):**

- Editing system prompts  
- Insights heatmap  
- Auto-learn theory  
- Injection method deep-dives (default clipboard paste is fine)  
- “Launch at startup” before keys work  

---

### 5.2 Information architecture (nav)

#### Current (wrong for first-run)

```
General  ← default (startup checkboxes)
Dictation
Hotkeys
Dictionary
Cloud Keys   ← keys buried here
Prompts
Output
History
Insights
About
```

#### Target (happy path first)

```
Setup          ← NEW default tab (Ready + keys + mic + hotkeys + how-to)
Dictation      ← cleanup level (can also show summary on Setup)
Hotkeys        ← full capture UI (summary on Setup is enough for most)
Dictionary
Cloud Keys     ← keep for multi-key reorder / endpoint; Setup has the essentials
Prompts        ← advanced
Output         ← advanced
History
Insights
General        ← demote (startup, tray, notifications)
About
```

| ID | Status | Effort | Work |
|----|--------|--------|------|
| **U2** | done | S–M | Reorder nav in `app/ui/index.html`; set default active tab to `setup`; move/duplicate essential controls onto Setup |

**Files:** `app/ui/index.html` (nav + new `tab-setup`), `app/ui/src/main.ts` (`activeTab`, tab switch, populate/gather).

---

### 5.3 Setup tab layout (U1) — what to build

Wireframe (content, not pixels):

```
┌─────────────────────────────────────────────────────────┐
│  Setup                                                   │
│  Get dictation working. Advanced options are in other    │
│  tabs.                                                   │
│                                                          │
│  ┌─ Ready ─────────────────────────────────────────────┐ │
│  │  ● Ready to dictate     or  ● Needs setup           │ │
│  │  [✓] ElevenLabs API key                             │ │
│  │  [✓] Groq API key        (hide/skip if cleanup=none)│ │
│  │  [✓] Microphone                                     │ │
│  │  [✓] Hotkeys valid                                  │ │
│  │  Last error: (none) / “STT open failed: …”          │ │
│  │  Engine: Idle | Recording | Processing | …          │ │
│  └─────────────────────────────────────────────────────┘ │
│                                                          │
│  ┌─ How to use ────────────────────────────────────────┐ │
│  │  1. Click in any text field (e.g. Notepad).         │ │
│  │  2. Hold Ctrl+Shift+Z, speak, release.              │ │
│  │  3. Polished text is pasted at the cursor.          │ │
│  │  Command: hold Ctrl+Shift+X …                       │ │
│  │  (strings update live from current hotkey settings) │ │
│  └─────────────────────────────────────────────────────┘ │
│                                                          │
│  ┌─ API keys ──────────────────────────────────────────┐ │
│  │  ElevenLabs: [add key] [list masked] reorder/remove │ │
│  │  Get a key: https://elevenlabs.io …                 │ │
│  │  Groq: [•••• password field]                        │ │
│  │  Get a key: https://console.groq.com …              │ │
│  │  Model: [dropdown] [Refresh models]                 │ │
│  └─────────────────────────────────────────────────────┘ │
│                                                          │
│  ┌─ Microphone ────────────────────────────────────────┐ │
│  │  [System default ▼]                                 │ │
│  └─────────────────────────────────────────────────────┘ │
│                                                          │
│  ┌─ Hotkeys (read-only summary + “Change…” → Hotkeys)  │ │
│  │  Dictation: Ctrl+Shift+Z                            │ │
│  │  Command:   Ctrl+Shift+X                            │ │
│  │  or: full capture widgets inline (preferred if easy)│ │
│  └─────────────────────────────────────────────────────┘ │
│                                                          │
│  ┌─ Cleanup (optional on Setup) ───────────────────────┐ │
│  │  ( ) none  ( ) light  (•) medium  ( ) high          │ │
│  └─────────────────────────────────────────────────────┘ │
│                                                          │
│  [ Save & apply ]   or auto-apply essentials (see U4)    │
└─────────────────────────────────────────────────────────┘
```

| ID | Status | Effort | Work |
|----|--------|--------|------|
| **U1** | done | M | New Setup tab HTML + Ready computation + how-to + embed keys/mic/hotkeys/cleanup |

**Ready logic (frontend, pure):**

```text
elevenlabs_ok = stt.api_keys has ≥1 non-empty key
groq_ok       = llm.api_key non-empty OR cleanup_level == "none"
mic_ok        = device list non-empty OR system_default always ok if host has default
hotkeys_ok    = three combos parse + unique + each has a modifier
ready         = elevenlabs_ok && groq_ok && mic_ok && hotkeys_ok
```

Show red/amber banner when `!ready` with the first failing item as the CTA (“Add an ElevenLabs key below”).

---

### 5.4 Keys UX (U3)

| Issue today | Fix |
|-------------|-----|
| Keys only under “Cloud Keys” | Essentials on **Setup**; keep Cloud Keys for endpoint, multi-key order, advanced |
| Empty keys → fail only on first hold | Ready checklist turns red **before** first hold |
| Groq password field OK; EL list OK | Keep masking; never log keys |
| No “get key” links on happy path | Add doc links next to each field |
| Model list requires Refresh + key | On Setup: after Groq key saved, offer Refresh; if fail, show friendly error on Setup |
| User changes keys but forgets Save | **U4** — Save & apply on Setup must be obvious |

**Validation copy examples:**

- “Add at least one ElevenLabs API key to use speech-to-text.”
- “Add a Groq API key for cleanup, or set cleanup to None (raw transcript only).”
- “Hotkeys must include Ctrl/Shift/Alt/Win and must all be different.”

| ID | Status | Effort |
|----|--------|--------|
| **U3** | done | S–M |

---

### 5.5 Save / apply model (U4)

| Issue today | Fix |
|-------------|-----|
| Global dirty save bar; easy to change mic/keys and never save | On **Setup**: primary button **Save & apply** that calls `save_settings` + engine `ApplySettings` + `set_autostart` if needed |
| Dictionary saves immediately; settings do not | Document in UI: “Dictionary saves immediately; Setup needs Save & apply” **or** auto-save essentials on Setup blur/change (debounce 400ms) |
| Save fails (invalid hotkey) only toast | Show error **inline on Setup** + toast |

**Recommended v1 approach (pick one):**

| Option | Pros | Cons |
|--------|------|------|
| **A. Explicit “Save & apply” on Setup** (recommended) | Clear, matches engine ApplySettings | One extra click |
| **B. Auto-save Setup fields** | Fewer mistakes | Harder to discard; more IPC |

Do **not** require visiting General or Cloud Keys to complete first-run.

| ID | Status | Effort |
|----|--------|--------|
| **U4** | done | S–M |

**Files:** `app/ui/src/main.ts` (`saveConfirmBtn`, `gatherFormSettings`, Setup-specific save), `app/src-tauri/src/main.rs` (`save_settings` already applies engine).

---

### 5.6 Status, errors, engine feedback (U5)

| Surface | Today | Target |
|---------|--------|--------|
| Sidebar badge | Idle / Recording / … | Keep; also show **Needs setup** when `!ready` |
| Errors | Toast + notification + tray tooltip | **Also** `last_error` string on Setup (from `engine-error` event) until next success |
| Not ready + user holds hotkey | STT open error | Prefer early engine check: if no keys, emit clear error *and* leave Setup banner; optional: do not show “Recording” |
| Success | History grows silently | Optional tiny “Last dictation: N words” on Setup (Phase 1 nice-to-have) |

Listen to existing events: `engine-state`, `engine-error` (already in `main.ts`).

| ID | Status | Effort |
|----|--------|--------|
| **U5** | done | S |

---

### 5.7 Overlay UX for the happy path (U6)

| State | Label | Notes |
|-------|--------|------|
| STT/mic starting | **Connecting…** | Only if open takes noticeable time; avoids lying with “Recording” before mic (ties to A1) |
| Capturing | **Recording** | Pulse from RMS |
| After release | **Processing** | Until inject done |
| Command edit | **Command** or **Edit** | If B1 dual mode |
| Command generate | **Generate** | If B1 dual mode |
| Soft failure | Toast on overlay | “No speech detected”, “Select text first”, etc. — already partially there |

| ID | Status | Effort | Depends |
|----|--------|--------|---------|
| **U6** | done | S–M | A1 (real Recording), B1 (mode labels) |

**Files:** `crates/vf-engine/src/overlay.rs`, orchestrator state strings.

---

### 5.8 Tray & first-run window (U7)

| Issue | Fix |
|-------|-----|
| `start_minimized` + first run = tray-only, user thinks app didn’t start | **First run** (no keys yet, or `settings` flag `first_run_done`): always show main window on Setup. Only honor start_minimized after ready once. |
| Tray menu: Open / Quit | Keep; tooltip: `VillFlow – Ready` / `Needs setup` / state |
| Close window = hide to tray | Keep (correct for tray app); Setup copy: “Closing hides to tray — Quit from tray menu.” |

Optional settings flag: `general.onboarding_complete` or derive from `ready && user_saved_once`.

| ID | Status | Effort |
|----|--------|--------|
| **U7** | done | S–M |

**Files:** `app/src-tauri/src/main.rs` (show/hide on startup), `app/ui` copy, maybe `vf-core` settings field if you persist onboarding.

---

### 5.9 Hotkeys on the happy path (U8)

| Issue | Fix |
|-------|-----|
| Hotkeys only on separate tab | Show current combos on Setup; either inline recorders or “Edit hotkeys” jump to Hotkeys tab |
| Capture UX clears field on focus | Keep; on Setup after save, refresh How-to text with new combos |
| Invalid bare key | Already rejected — surface error on Setup |

| ID | Status | Effort |
|----|--------|--------|
| **U8** | done | S |

---

### 5.10 In-app help copy (U9)

Short, always visible on Setup (not only README):

1. Hold **{dictation}**, speak, release → text at cursor.  
2. **{command}** → rewrite selection, or generate if nothing selected *(wording depends on B1)*.  
4. Keys stay in `%APPDATA%\VillFlow\` on this PC only.  
5. Need help building? README link.

| ID | Status | Effort |
|----|--------|--------|
| **U9** | done | S |

---

### 5.11 What to leave as advanced (do not block happy path)

| Tab | Role after Setup exists |
|-----|-------------------------|
| **Cloud Keys** | Endpoint host, multi-key reorder detail, full EL list management if Setup has “add one key” only |
| **Dictation** | Full cleanup explanations; mic can stay duplicated |
| **Hotkeys** | Full capture UI if Setup only summarizes |
| **Prompts** | Power users only — collapse or leave last |
| **Output** | Injection method + restore clipboard |
| **Dictionary** | Spelling + auto-learn |
| **History / Insights** | After user has dictated |
| **General** | Startup, minimized, notifications |
| **About** | Version, links |

**Visual redesign (D3):** not required. Reuse existing cards, dark theme, save bar styles for Setup.

**Scratchpad (D4):** removed — AppInsert only for Settings fields when VillFlow is focused (C3).

---

### 5.12 Settings schema / backend touchpoints for UI

| Need | Backend already? | UI work |
|------|------------------|---------|
| get/save settings | Yes | Wire Setup fields into `gatherFormSettings` / `populateForm` |
| ApplySettings on save | Yes | Call from Setup Save & apply |
| list_input_devices | Yes | Mic dropdown on Setup |
| list_groq_models | Yes | Refresh on Setup |
| engine-state / engine-error | Yes | Ready badge + last error |
| first-run force show window | Partial (`start_minimized`) | Shell logic U7 |
| “test mic” button | No | **Optional Phase 1** — not required if A1 + Recording pulse works |
| “test dictation” | No | **Optional** — user uses Notepad; don’t overbuild |

No new cloud APIs required for Phase 0 UI.

---

### 5.13 Implementation order for UI/UX (within Phase 0)

| Order | ID | Task | Effort |
|------:|----|------|--------|
| 1 | U2 | Nav: add Setup, default tab, demote General | S |
| 2 | U1 | Setup layout + Ready checklist | M |
| 3 | U3 | Keys + links on Setup | S |
| 4 | U8 + U9 | Hotkeys summary + How to use | S |
| 5 | U4 | Save & apply essentials | S–M |
| 6 | U5 | Last error + Needs setup badge | S |
| 7 | U7 | First-run show main window | S |
| 8 | U6 | Overlay Connecting + mode labels | S–M (with A1/B1) |

**Parallel with engine:** U* can ship even before A1, but Ready should not say “Recording works” until A1 is fixed — copy can say “Hold hotkey in Notepad to try.”

---

### 5.14 Happy-path UI exit criteria (Phase 0 UI)

```text
[ ] Fresh settings (no keys): main window opens on Setup, Ready = Needs setup
[ ] User can add EL + Groq keys on Setup without opening Cloud Keys tab
[ ] Save & apply persists keys; engine gets ApplySettings; Ready turns green
[ ] How-to shows actual hotkey strings from settings
[ ] engine-error appears as Last error on Setup
[ ] Closing window still tray-hides; Quit only from tray
[ ] After ready, hold hotkey → user understands from overlay + How-to what to do
[ ] Advanced tabs still work (History, Prompts, etc.)
[ ] No redesign required — existing dark theme / components reused
```

---

### 5.15 UI issue register (tracking)

| ID | Title | Status | Severity | Phase |
|----|-------|--------|----------|-------|
| U1 | Setup tab + Ready checklist | **done** | High | 0 |
| U2 | Nav order / default Setup | **done** | High | 0 |
| U3 | Keys UX on Setup | **done** | High | 0 |
| U4 | Save & apply essentials | **done** | High | 0 |
| U5 | Status / last error / not-ready | **done** | High | 0 |
| U6 | Overlay Connecting + mode | **done** | Medium | 0–1 |
| U7 | Tray + first-run window | **done** | High | 0 |
| U8 | Hotkeys on Setup | **done** | Medium | 0 |
| U9 | How-to copy on Setup | **done** | Medium | 0 |

---

## 6. Phased roadmap

### Phase 0 — “GitHub release is usable” (engine + happy-path UI)

| Order | ID | Item | Status |
|------:|----|------|--------|
| 1 | A1 | Mic from key-down; STT open in parallel | **done** |
| 2 | **U1–U5, U7–U9** | Setup tab package | **done** |
| 3 | A3 | Release + build docs (portable + installer) | **done** |
| 4 | A4 | Remove self-inflicted latency | **done** |
| 5 | A5 | Sticky last-good STT key | **done** |
| 6 | U6 | Overlay Connecting + Edit/Generate | **done** |
| 7 | B4 + F1 | Honest latency + PRODUCT.md | **done** |

**Exit criteria:** met (user-tested 2026-07-15).

### Phase 1 — “Command mode & trust”

| Order | ID | Item | Status |
|------:|----|------|--------|
| 1 | B1 | Dual command mode + overlay labels | **done** |
| 2 | B3 | Field context off by default | **done** |
| 3 | C2 | Safer selection (toast on edit→generate) | **done** |
| 4 | B2 | History delete/clear allowed | **done** |
| 5 | F3 | Manual test checklist in CONTRIBUTING | **done** |
| 6 | — | Last dictation on Setup + timing logs | **done** |

### Phase 2 — “Nice for the community”

| ID | Item | Status |
|----|------|--------|
| D6 | speech_ms for WPM | **done** |
| C6 | Overlay on focused monitor | **done** |
| C7 | Partial STT on overlay | **done** |
| E2 | History retention days | **done** |
| D5 | Basic a11y + issue templates | **done** |
| E1 | Key vault (DPAPI + optional passphrase) | **done** |
| — | README badge OWNER | deferred until real GitHub repo URL known |

### Phase 3 — security & polish

| ID | Item | Status |
|----|------|--------|
| E1 | DPAPI encrypt API keys at rest | **done** |
| — | History export JSON | **done** |
| — | Setup mic test + show/hide Groq key | **done** |
| — | PR template + SECURITY DPAPI | **done** |
| — | README badge OWNER | deferred until real GitHub URL |

### Leave until someone files an issue

- D3 visual redesign, telemetry, accounts (D4 Scratchpad removed)  
- Cross-user key migration / portable encrypted vault beyond DPAPI  

---

### Phase 4 — release complete

| Item | Status |
|------|--------|
| Real GitHub URLs in README | **done** (`SreekarGpalli/VillflowV3`) |
| Release CI: portable + installers | **done** |
| Scratchpad | **removed** |
| Dictionary export | **done** |
| About page | **done** |

### Phase status summary

| Phase | Status |
|-------|--------|
| 0 Happy path | **done** (user-tested) |
| 1 Command trust | **done** |
| 2 Community | **done** |
| 3 Security/polish | **done** |
| 4 Release complete | **done** |
| 5 UI redesign + portable vault; Scratchpad removed | **done** |

**Non-goals (will not build):** accounts, telemetry, commercial packaging beyond portable+installer, full Wispr parity, multi-language v1.

---

## 7. Implementation patterns (when coding)

| Area | Pattern |
|------|---------|
| Core loop | Capture ∥ STT open; UI state matches reality |
| Config / UI | **Setup-first**; keys + mic + hotkeys + Ready on one screen (§5) |
| Save | Clear **Save & apply** for essentials (or auto-save Setup only) |
| Release | One documented command → one `villflow.exe` on Releases |
| Latency | No multi-hundred-ms sleeps on happy path; log timings |
| LLM | Predictable cleanup over clever multi-retry |
| Command mode | Visible mode or separate hotkeys; never silent wrong mode |
| Docs | README for humans; short product rules you believe; **How-to also in-app** |
| Privacy | Local only; clear history; document plaintext keys |
| Injection | Best-effort; document app limits |

### Suggested verification after Phase 0

```text
Engine
[ ] tauri build (or script) produces working UI without Vite
[ ] Hold dictation hotkey, speak from first moment → full phrase in Notepad
[ ] Release → text appears without long artificial pause (cleanup none)
[ ] With two ElevenLabs keys (first bad): recovery then next utterance uses good key
[ ] No API keys in log file

Settings / UI / UX (§5.14)
[ ] Fresh install opens Setup, not General
[ ] Empty keys → Needs setup (not only a late STT error)
[ ] Keys entered on Setup + Save & apply → Ready
[ ] How-to shows real hotkeys
[ ] Last error visible on Setup after a failure
```

### Manual regression checklist (expand in CONTRIBUTING)

```text
[ ] Dictation → Notepad
[ ] Dictation → browser text field
[ ] Command with selection → rewrite
[ ] Command without selection → (per B1) toast or generate
[ ] Change hotkey on Setup/Hotkeys, save, re-test
[ ] Quit from tray cleans up (no stuck modifiers)
[ ] First run: window visible; second run with start_minimized: OK after onboarding
```

---

## 8. Quick reference — fix order cheat sheet

```
P0 Engine
  A1 Mic timing
  A3 Ship path
  A4 Latency self-delays
  A5 Sticky STT key

P0 Happy path UI/UX (§5)  ← do not skip
  U2 Nav → Setup default
  U1 Setup + Ready checklist
  U3 Keys on Setup
  U4 Save & apply
  U5 Status / last error
  U7 First-run window
  U8 Hotkeys on Setup
  U9 How-to copy
  U6 Overlay Connecting (+ modes)

P1 Product + trust
  B1 Command mode decision + UX
  B3 LLM context simplification
  C2 Selection safety
  F1 Docs/contracts truth

P2 Community polish
  Metrics, overlay multi-monitor, partials, retention, encryption, a11y

Skip for v1
  Brand redesign, commercial onboarding, hard 700ms SLA,
  installer-required packaging, Wispr parity
```

---

## 9. Code map (where to look)

| Concern | Primary paths |
|---------|----------------|
| Orchestrator / PTT flow | `crates/vf-engine/src/orchestrator.rs` |
| Hotkeys | `crates/vf-engine/src/hotkeys.rs` |
| Audio capture | `crates/vf-engine/src/audio.rs` |
| UIA / selection | `crates/vf-engine/src/context.rs` |
| Injection | `crates/vf-engine/src/inject.rs` |
| Overlay | `crates/vf-engine/src/overlay.rs` |
| Auto-learn | `crates/vf-engine/src/autolearn.rs` |
| STT + key rotation | `crates/vf-cloud/src/stt.rs` |
| Groq | `crates/vf-cloud/src/groq.rs` |
| Prompts | `crates/vf-cloud/src/prompt.rs`, `crates/vf-core/src/lib.rs` |
| Sanitize / echo | `crates/vf-cloud/src/sanitize.rs` |
| Settings / SQLite | `crates/vf-store/src/lib.rs` |
| Types / defaults | `crates/vf-core/src/lib.rs` |
| Tauri shell / tray | `app/src-tauri/src/main.rs` |
| Settings UI | `app/ui/src/main.ts`, `app/ui/index.html` |
| Prior verify notes | `docs/internal/VERIFY-REPORT.md` |
| **Happy-path UI plan** | **This file §5** |

---

## 10. Document history

| Date | Change |
|------|--------|
| 2026-07-15 | Initial full critical re-review under OSS / free / no-accounts context; decision matrix + phases |
| 2026-07-15 | Added **§5 Happy path Settings/UI/UX** (U1–U9) |
| 2026-07-15 | **Owner decision session:** locked PRODUCT.md; updated CONTRACTS + this file §0; ready to implement Phase 0 |
| 2026-07-15 | Phase 0 implemented + user-tested OK; Phase 1 completed (C2 toast, timings, last dictation, CONTRIBUTING checklist) |
| 2026-07-16 | Scratchpad feature removed (dictation-only product) |
| 2026-07-15 | Phase 2: speech_ms WPM, multi-monitor overlay, partials, history retention, a11y, issue templates |
| 2026-07-15 | Phase 3: DPAPI keys, history export, mic test, PR template — planned phases 0–3 complete |

When you complete fixes, update **Status** fields in §4 / §5.15 and add a row here.

---

## 11. Bottom line

For an open-source, free, no-account, GitHub-download dictation app:

**Must fix (engine):** speech capture timing, release binary, sticky keys, avoid self-inflicted slowness.

**Must fix (settings/UI/UX — §5):** Setup-first tab, Ready checklist, keys on first screen, Save & apply, status/last error, first-run window, in-app how-to. Do **not** treat UI as optional polish after the engine — without it the happy path is still broken for GitHub users.

**Keep / defer:** dense advanced tabs (Prompts, Insights, etc.), brand redesign, installers, Wispr-level polish.

**Drop from “requirements”:** hard 700ms SLA, forbidding history delete, commercial assumptions.

Work **Phase 0** (engine **+** §5 UI) until: Setup → keys → Ready → Notepad dictation is boringly reliable.
