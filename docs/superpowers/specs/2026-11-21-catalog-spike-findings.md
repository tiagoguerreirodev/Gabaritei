# Catalog and Fast Mode spike findings

Date: 2026-11-21. Claude Code 2.1.330, macOS. Status: **partial**. Claude is verified. The OpenAI sign-in, the account catalog and Codex Fast Mode are not verified yet (see "Not verified").

## Claude Code (verified)

### Model list (plan Step 3)

- `claude --help` has no command or flag that lists models. `--model` takes an alias (`fable`, `opus`, `sonnet`, the help text's examples) or a full name.
- `claude models` is not a command: it is read as a prompt and sends a real request. Don't probe with it.
- With `claude -p ... --output-format stream-json --verbose`, the `system`/`init` event has `model`, the full name an alias resolved to (`opus` gave `claude-opus-5-5`, `sonnet` gave `claude-sonnet-5-5`). Resolving an alias costs a real request each time (about $0.003 to $0.05 here), so it's not a good way to build a list.
- Nothing in the init event, the `result` event or `--help` lists models or per-model efforts. The only effort data is the fixed `--effort` help text (`low, medium, high, xhigh, max`).

**Conclusion:** Claude has no listing source. The Claude source returns `Unavailable` with the pt-BR reason "O Claude Code não informa a lista de modelos; digite o nome.", and Model and Effort stay free text for Claude, as the design's fallback says. Effort names for Claude can come from the `--effort` help text, if we choose to parse it later (not planned: fragile).

### Fast Mode (plan Step 4, Claude half)

- The `system`/`init` event and the final `result` event both carry `fast_mode_state` (`on` or `off`) and `fast_mode_disabled_reason`.
- Default (no setting), `--model opus`: init `off`, reason `sdk_opt_in_required`. So print mode needs an opt-in.
- `--settings '{"fastMode":true}'` together with `--setting-sources ""` and `--strict-mcp-config` (the isolation flags the app uses) with `--model opus`: init `on`. So the setting is read with isolation in place.
- The same setting with `--model sonnet`: init stays `off` with no reason, so Sonnet does not support Fast Mode. The init event is therefore a way to tell whether a model supports it, but only after a request.
- On this account the final `result` for the opus run says `off` with reason `extra_usage_disabled`. Fast Mode needs extra usage enabled on the Claude account, otherwise a run silently proceeds without it.

**Conclusion (Claude):** the mechanism is `--settings '{"fastMode":true}'`. Support per model can't be listed ahead of time; it is only visible per run (`fast_mode_state`/`fast_mode_disabled_reason`). The "only when the model supports it" checkbox cannot be fed from a catalog for Claude. It needs a design decision (see Open questions).

## Not verified

- **Step 1, own sign-in and token:** not run. It needs the owner to sign in with their OpenAI account in a browser, and the legacy flow's details (client id, scope, redirect, `docs/legacy-analysis.md` D1 and N-B1) are only summarized in this repo. The legacy clone `../Simulados-Concursos` is not present here.
- **Step 2, account model list:** depends on Step 1.
- **Step 4, Codex Fast Mode:** `codex` is not installed on this Mac, so no `-c` key or `--strict-config` test could run. It needs the Windows machine or a Codex install here.

## Open questions

1. How should the Fast Mode checkbox appear for Claude, where support can't be known before a run? Options: show it for every Claude model and report the result per run, or only for model names the owner lists, or hide it for Claude.
2. Who runs Steps 1, 2 and the Codex part of 4, and where (this Mac with Codex installed, or the Windows machine)? Step 1 involves signing in with the owner's account, so it needs their interactive approval.
