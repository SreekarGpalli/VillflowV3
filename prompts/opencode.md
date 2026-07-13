# opencode brief — verifier (run after every phase)

You verify; you do not build features. Keep changes minimal and mechanical.

## Verification pass (replace <N> with the phase you are checking: P1…P5)

1. Read `CONTRACTS.md` §4, §16, §17, and the section(s) the phase implements (P1→§10–§12; P2→§6–§9; P3→§5,§15; P4→§13–§14; P5→§5,§13).
2. Run, in order, from the repo root:
   - `cargo build --workspace`
   - `cargo clippy --workspace`
   - `cargo test --workspace`
   - For P4/P5 additionally: `npm run build` inside `app/ui`. For V5 additionally: `cargo build --release --workspace`.
3. Review `git show --stat HEAD` and the diff of the phase commit. Check, mechanically:
   - Only crates owned by that agent were touched (CONTRACTS §4).
   - No dependencies outside the §3 whitelist were added.
   - No §2 non-goal features appeared; no telemetry; no API keys logged.
   - Every `// TODO(vf)` in the diff is listed in a summary/report.
   - Settings fields, DB columns, IPC command names match §10, §11, §13 exactly.
4. Fixes you may apply yourself: compile errors, clippy warnings, typos, missing derives — up to 30 changed lines TOTAL. Anything larger: do not fix; report it.
5. Write findings to `VERIFY-REPORT.md` (append a section):
   ```
   ## V<N> — <date>
   build: PASS/FAIL   clippy: PASS/n warnings   tests: PASS/FAIL (n tests)
   findings:
   - [severity: blocker|minor] <file>: <one-line finding>
   fixes applied: <list or none>
   ```
6. Commit: `opencode: V<N>: verify <phase>` (include VERIFY-REPORT.md and any fixes).

Do not refactor, do not restructure, do not add features, do not edit CONTRACTS.md or prompts/.
