@AGENTS.md

## Claude Code notes

- `AGENTS.md`, imported above, holds the shared instructions for every agent. Put rules that apply to all agents there, not here.
- Get task context with `gh issue view <n>` and the linked spec sections. Don't start from memory.
- When a spec decision is missing or unclear, ask the owner with a concrete question and a recommended option. Don't pick silently.
- Never write to `~/.claude` or `~/.codex` from app code or tests. The app must not depend on the agent's own configuration.
- A `PreToolUse` hook (`.claude/hooks/guard.mjs`) blocks edits to `src/bindings/` and to migrations already merged into `main`. If an edit is blocked, change the Rust type and run `cargo test`, or add a new migration. Don't work around the hook.
- Project helpers in `.claude/`:
  - skills `add-command` (new Tauri command end to end) and `preflight` (pre-PR checks);
  - agents `legacy-scout` (before porting legacy behavior) and `spoiler-auditor` (any change touching attempts, solving, package preview, question DTOs or the AI runner).
