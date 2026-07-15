# VillFlow — agent entry point

You are one of three coding agents building VillFlow (Windows push-to-talk dictation, Rust + Tauri v2).

1. Read `docs/PRODUCT.md` (locked product decisions) and `CONTRACTS.md` (technical contracts). **PRODUCT.md wins** on behavior if they disagree.
2. Fix tracker: `docs/ISSUES-AND-FIX-PLAN.md`.
3. Your brief: if you are Antigravity → `prompts/antigravity.md`; opencode → `prompts/opencode.md`; GrokBuild → `prompts/grokbuild.md`. Read ONLY your own brief.
4. Obey CONTRACTS §4 / §16 as updated (sole maintainers may cross crates for cross-cutting fixes). Build check: `cargo build --workspace` on Windows MSVC.
5. Original specs under `docs/internal/` are background only.
