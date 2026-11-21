# Model, Effort and Fast Mode options fed by the agents' own sources: design

Status: approved 2026-11-21, amended the same day after the spike (see "Amendment").

## Intent

In "Funções" (Configurações) and "Gerar", the Model and Effort selectors are hard-coded: Model is a free-text box, Effort is a fixed list in `src/pages/settings/labels.ts`, and the default model lives in `roles.rs`. The owner wants those options, plus a new **Fast Mode**, to come from the agents' own sources, so the app shows what the owner can really use.

- **Codex:** from the connected OpenAI subscription account, which exposes more than the CLI (models, reasoning levels, Fast Mode tiers).
- **Claude:** from the Claude Code CLI.

## Decisions (owner)

- The RemNote token issue is closed (the user was sending `Bearer` twice). Out of this work.
- Load approach **A**: fetched on demand the first time "Funções" or "Gerar" opens, cached in memory until the app closes, with an "Atualizar" button.
- Claude has no model-listing command (`claude --help` shows only `--model <alias or full name>` and `--effort`). The plan probes the CLI; if nothing works, Claude's Model is free text.
- Effort options depend on the selected model, from what the source reports. If it reports none, fall back to the source's own list of effort names.
- If a source is unavailable (CLI missing, not signed in, request fails): pt-BR warning and free-text Model and Effort. The owner is never blocked.
- A saved model that the source no longer lists is kept, shown as "não disponível", and runs are blocked until another is picked.
- Fast Mode: a "Modo rápido" checkbox per role, shown only when the selected model supports it. Saved with the role; overridable in "Gerar". The mechanism for turning it on is verified on the real tools, recorded in `docs/ai-cli.md`; an agent with no working mechanism gets no option.
- Fast Mode use is **not** recorded in package notes or provenance.
- **AGENTS.md rule 2 is amended** (owner-approved, 2026-11-21). The app may use the OpenAI subscription account through the app's **own sign-in (OAuth)**, with the token in the OS keychain like the RemNote token. Claude still goes only through the local `claude` CLI. The spec's §6 entries for ChatGPT OAuth and the model catalog are narrowed accordingly.
- The work is split: a spike, then PR 1 (catalog), then PR 2 (Fast Mode). **Codex runs through the account** (replacing `codex exec`) is a separate later project and needs its own design.

## Goals

- Model and Effort options always reflect the real source; nothing about available models is hard-coded in the UI.
- The owner can sign in to the OpenAI account from the app, and sign out.
- Fast Mode works for both agents where supported.

## Non-goals

- Running Codex through the account (later project). Until then Codex runs still use `codex exec`.
- Recording Fast Mode in provenance.
- Any model catalog beyond Model, Effort and Fast Mode (no pricing, no context sizes).

## Anti-goals

- The Codex token must never be logged, put in arguments, exports, backups or git, and must never be shown to the frontend.
- Question content, prompts and answer keys are untouched by this work (no-spoiler and blind-check rules are unaffected).
- No guessing a Fast Mode mechanism: if it can't be verified it isn't shipped.

## Approaches considered (loading the lists)

- **A. On demand, in-memory cache, "Atualizar" (chosen).** Simple; always from the live source; no schema change.
- B. Fetch at startup into SQLite. Instant and offline, but needs a migration and a staleness rule.
- C. Fetch on every open, no cache. Simplest, but slow.

## Plan of work

### Step 0: Spike (throwaway, no tests, not merged)

Questions it must answer, recorded in a short findings note (kept as a doc, the code discarded):

1. Can an app with its **own sign-in** obtain an OpenAI subscription token? The legacy flow used a special client id and scope (legacy decision D1) that may not work for a new client.
2. With that token, can the app read the model list including reasoning levels and service tiers (legacy N-B2: `supported_reasoning_levels`, `service_tiers`)?
3. Can `claude` report its models or per-model effort in any way (probe `--help`, `claude -p` init events, settings)? What is the best source for Claude?
4. How is Fast Mode turned on for Claude (e.g. a `fastMode` setting through `--settings`) and for Codex (e.g. a `service_tier` config key)? Each verified with a real run, with `--strict-config` for Codex.

Gate: if question 1 fails, PR 1 is redesigned (back to the owner) before any code.

### PR 1: Catalog and sign-in

- Amend `AGENTS.md` rule 2 and `docs/spec.md` §2, §4, §6 (spec changes in the same PR).
- Sign-in to the OpenAI account in the app, token stored in the OS keychain next to the RemNote token (see `src-tauri/src/ai/secrets.rs`); sign-out; status in Configurações.
- A backend module that returns, per agent, models and, per model, supported efforts, with an in-memory cache and a refresh.
- A Tauri command (`add-command` skill), `api.ts` function and regenerated bindings.
- Replace the free-text Model and fixed Effort in `RolesSection.tsx` and `RolePicker.tsx` with selectors fed by the command, with the fallback and "não disponível" behaviors above.
- Tests: fake catalog sources (no real network or CLI calls in tests), stale-model blocking, fallback to free text.

### PR 2: Fast Mode

- A `fast` flag in each role's saved config (stored as JSON in `settings`, so no migration expected).
- Runner support for each agent using the mechanism verified in Step 0.
- Checkbox shown only when the catalog says the model supports it.
- `docs/ai-cli.md` updated with the verified mechanisms.

### Later project: Codex runs through the account

Replaces `codex exec`. Requires rebuilding the read-only tool loop, attachments, RemNote tools, web search and the blind-check isolation inside the app. Spec §6 currently excludes the in-app tool loop and the PDF extractor, so it needs its own brainstorm and spec changes.

## Calls I made (owner can overrule)

- Sign-in lives in Configurações, with a pt-BR "Entrar com a conta OpenAI" / "Sair" control. The exact flow (browser plus local redirect, or device code) is decided after the spike.
- The first-run default role model (`gpt-6.2-sol`, extra-high) stays in `roles.rs` as a starting value, not a selector list. It is validated against the catalog like any saved model.
- When the account is not signed in, Codex falls back to free text with a warning (same as any failed source).
- Cache is per app session, shared by "Funções" and "Gerar".

## Left to the builder

Module and type names, exact pt-BR strings, the cache structure, and the OAuth flow variant once the spike answers question 1.

## Amendment (after the spike, owner-approved 2026-11-21)

The service refused the app's own OpenAI sign-in (see the findings note), so these parts above are replaced:

- **Codex source:** `codex debug models` (the CLI's own catalog), not the account. No sign-in, no OAuth, no token in the keychain. **AGENTS.md rule 2 is not amended**; only spec §6's "model catalog" entry is narrowed (the legacy HTTP catalog stays out).
- **Plan changes:** PR 1 loses the sign-in and its Configurações section; the Codex source parses `codex debug models`.
- **Effort:** `Effort` gains `Ultra` (Codex only).
- **Claude Fast Mode:** the checkbox is always shown for Claude (support can't be known before a run). For Codex it is shown only for models whose `service_tiers` is non-empty.
- **Fast Mode mechanisms:** Claude `--settings '{"fastMode":true}'`; Codex `-c service_tier="priority"` (key verified, effect unobserved).
- The later "Codex runs through the account" project is dropped unless the owner reopens it, since the sign-in it needed is refused.
