# Catalog and Fast Mode spike findings

Date: 2026-11-21. Claude Code 2.1.330, macOS. Status: **partial, and the gate failed**. Claude is verified. The OpenAI sign-in (Step 1) was refused by the service, so the account catalog and Codex Fast Mode could not be tested (see "OpenAI sign-in").

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

## OpenAI sign-in (Step 1): refused

- The legacy flow (`dynamic_agent_client`, scope `chatgpt.tokens.use.direct`, local redirect, PKCE; legacy `electron/connections.ts`) was run by the owner with a throwaway script. The browser showed "invalid authorize request".
- Reproduced without a browser by requesting the authorize URL directly: HTTP 400, `{"error":{"message":"Invalid authorize request","code":"invalid_authorize_request"}}`.
- The service's public metadata (`https://auth.openai.com/.well-known/openid-configuration`, 2026-11-21) lists **no `registration_endpoint`** (no dynamic client registration) and **no device authorization endpoint**. `scopes_supported` is only `openid`, `profile`, `email`, `offline_access`; the legacy scopes `resource.invoke` and `chatgpt.tokens.use.direct` are not listed. Grants: `authorization_code`, `refresh_token`. PKCE: `S256`.
- This is the risk legacy decision D1 warned about: a new app can't register itself the way the legacy flow assumed.
- Not tried, on purpose: borrowing another application's client id (for example the Codex CLI's), because that would present the app as a different client.

Consequence: Steps 2 (account model list) and 4 (Codex Fast Mode through the account) are blocked. From the legacy code, the account's Fast Mode was the request field `service_tier` (`priority` or `fast`) and effort was `reasoning.effort` (levels can include `none` and `minimal`), but this was not verified against the live service.

## Open questions

1. With the account sign-in refused, how should Codex's Model, Effort and Fast Mode options be sourced? This needs the owner (rule 2 and spec §6 were amended on the assumption that sign-in works).
2. Claude's Fast Mode checkbox: decided by the owner, always shown for Claude.
