# VillFlow Verification Report

## V1 — 2026-07-13
build: PASS   clippy: PASS (0 warnings)   tests: PASS (2 tests)
findings:
- [minor] crates/vf-core/Cargo.toml, crates/vf-store/Cargo.toml: dependency `chrono` is outside the CONTRACTS §3 allowed-crate whitelist. Used for RFC3339 timestamps (dictionary.created_at, history.ts, scratchpad.updated_at, insights 365-day cutoff). Flagging per §16 rule 2; not fixed (removing chrono exceeds the 30-line budget and would alter timestamp behavior).
- [minor] crates/vf-cloud/src/lib.rs, crates/vf-engine/src/lib.rs: P1 added 1-line stub files (`// GrokBuild owns this crate.`) in crates owned by another agent (§4). Necessary for `cargo build --workspace` to compile the empty members; files are clearly marked and contain no logic, so no ownership conflict at the code level.
- [info] No `// TODO(vf)` markers present in the P1 source diff.
- [info] Settings schema (§10), SQLite schema (§11), and vf-core surface (§12: enums, Settings, EngineEvent/Cmd/State, DictEntry, HistoryEntry, InsightsSummary, Store trait, default prompt consts, default_settings) all match the contract. No API keys logged; no §2 non-goals; no telemetry.
fixes applied: none (build, clippy, and tests already clean — no compile/clippy errors, typos, or missing derives to correct within budget).

## V2 — 2026-07-13
build: PASS   clippy: PASS (0 warnings)   tests: PASS (31 tests: 29 vf-cloud + 2 vf-store)
findings:
- [info] crates/vf-cloud/src/stt.rs: §6 warned the exact ElevenLabs wire field names were not pinned at contract time. P2 pins them (message_type, audio_base_64, input_audio_chunk, session_started/partial_transcript/committed_transcript(+_with_timestamps), auth_error/quota_exceeded/rate_limited/resource_exhausted) and documents confirmation from the ElevenLabs docs at stt.rs:3-4. Per CONTRACTS §6, re-verify against live docs during P3 integration; if any field name contradicts, record it in VERIFY-REPORT and stop — do not improvise.
- [info] Key rotation (KeyRotator) correctly enforces "at most one full cycle per utterance" and buffers audio for mid-utterance resend on rotatable errors; HTTP 401/403/429 at handshake also trigger rotation. Matches §6.
- [info] prompt.rs correctly returns None for CleanupLevel::None (LLM skipped, §8) and builds the exact §9 (system, user) shapes, including command INSTRUCTION:/TEXT: formatting and `(none)` substitution for empties. keyterms.rs enforces 50-term / 20-char caps with starred-first then use_count ordering (§6).
- [info] groq.rs matches §7: Bearer auth, temperature 0.2, max_completion_tokens 2048, non-streaming, choices[0].message.content trimmed + quote/fence stripping, GET /openai/v1/models → data[].id. API key is never logged (error snippets truncated, key never included).
- [info] No `// TODO(vf)` markers in P2 source. Only the GrokBuild-owned `vf-cloud` crate (plus Cargo.lock) was touched — crate-ownership §4 respected. All dependencies are within the §3 whitelist (tokio, tokio-tungstenite, futures-util, reqwest, serde, serde_json, thiserror, anyhow, base64, log). No §2 non-goals, no telemetry, no API keys logged.
fixes applied: none (build, clippy, and tests already clean — nothing to correct within the 30-line budget).
