---
name: preflight
description: Run every CI check locally before opening a PR, in CI order, stopping at the first failure.
disable-model-invocation: true
---

# Preflight

Run these from the repo root, one at a time. Stop at the first failure, show its output, and fix it (or report it) before going on. Don't claim success for a step you didn't run.

1. `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
2. `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path src-tauri/Cargo.toml`
4. Bindings up to date: `git status --porcelain -- src/bindings`. Any output means `cargo test` regenerated types that aren't committed. List them and tell the user to commit them.
5. `npm run lint`
6. `npm run typecheck`
7. `npm test`
8. Legacy bank test, only when the branch touches bank identity or the migration. Check `git diff main --name-only`. If it lists `src-tauri/src/bank/identity.rs` or `src-tauri/src/migrate.rs`, and `../Simulados-Concursos` exists, run:
   `LEGACY_CLONE=../Simulados-Concursos cargo test --manifest-path src-tauri/Cargo.toml real_legacy_bank -- --ignored --nocapture`
   If the clone is missing, say the test was skipped.

End with a short table: each step, then pass, fail or skipped.
