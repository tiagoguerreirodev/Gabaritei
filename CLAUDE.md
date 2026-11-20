@AGENTS.md

## Claude Code notes

- `AGENTS.md`, imported above, holds the shared instructions for every agent. Put rules that apply to all agents there, not here.
- Get task context with `gh issue view <n>` and the linked spec sections. Don't start from memory.
- When a spec decision is missing or unclear, ask the owner with a concrete question and a recommended option. Don't pick silently.
- Never write to `~/.claude` or `~/.codex` from app code or tests. The app must not depend on the agent's own configuration.
