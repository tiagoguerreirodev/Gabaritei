# CLI-fed Model, Effort and Fast Mode options: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Model, Effort and Fast Mode choices in "Funções" and "Gerar" come from the agents' real sources (OpenAI account for Codex, the `claude` CLI for Claude) instead of hard-coded values.

**Architecture:** A Rust `ai::catalog` module asks one `CatalogSource` per agent for its models (each with the efforts it supports), caches the answer in memory for the app session, and exposes it through one Tauri command. The frontend renders selectors from that command and falls back to free text when a source is unavailable. Codex's source uses an app-owned OpenAI sign-in whose token lives in the OS keychain.

**Tech Stack:** Rust (Tauri 2, `ureq`, `keyring`, `ts-rs`), Solid.js + TypeScript, Vitest.

**Spec:** `docs/superpowers/specs/2026-11-21-cli-fed-model-options-design.md`

## Global Constraints

- UI text is pt-BR only; code, comments, docs, commits and PRs are English.
- Commands are thin; domain logic is plain Rust tested without Tauri. Only `src/api.ts` calls `invoke`. After Rust type changes run `cargo test --manifest-path src-tauri/Cargo.toml` and commit the regenerated `src/bindings/` (never hand-edit them).
- Migrations are append-only. This work is expected to need none (role config is JSON in `settings`).
- No real AI, network or keychain calls in tests: use fake sources, fake CLI executables and `Secrets::memory()`.
- The OpenAI token is never logged, passed as an argument, exported, backed up, committed or sent to the frontend. Log with `log::error(..)` only, and never log study content or credentials.
- One issue per branch and PR, branch name `issue-<n>-<slug>`, Conventional Commits in English, `Closes #<n>`.
- Spec changes made here are updated in the same PR (AGENTS.md rule 7). The owner approved amending rule 2 on 2026-11-21.
- Before each PR: `cargo test`, `cargo clippy ... -D warnings`, `cargo fmt --check`, `npm run lint`, `npm run typecheck`, `npm test` (use the `preflight` skill). A change touching the AI runner or settings also gets the `spoiler-auditor` agent.

## Review Focus

1. **Effort name outside the `Effort` enum** (`low|medium|high|xhigh|max`) reported by a source: ignored, never a crash or a broken selector.
2. **Model listed with no efforts:** its Effort selector offers the enum values the source still reports for other models, or free text if none.
3. **Expired or revoked OpenAI token:** one failed refresh makes the account "not signed in" with a pt-BR warning and free text; no retry loop, no error toast storm.
4. **Switching a role's agent:** the model becomes the new agent's first listed model (or stays free text), never the old agent's model name.
5. **Two refreshes at once** (both pages open, or a double click on "Atualizar"): one request in flight, both callers get the same result.

---

## Step 0: Spike (throwaway, not merged)

### Task 0: Verify the four unknowns

**Files:**
- Create: `docs/superpowers/specs/2026-11-21-catalog-spike-findings.md` (kept)
- Throwaway code on branch `spike-catalog-sources` (discarded after)

**Interfaces:** Produces the findings note that fixes the sign-in flow, the Codex catalog endpoint and payload shape, the Claude source (or "none"), and the Fast Mode mechanism per agent. Tasks 4, 5 and 3 read it.

- [ ] **Step 1: Sign-in feasibility.** On the throwaway branch, run the legacy sign-in flow (see `docs/legacy-analysis.md` decision D1 and feature N-B1) from a small Rust or Node script with the app's own client. Record: which client id and scope work, redirect style (local browser redirect or device code), token and refresh lifetimes, and whether a new client is accepted. If it is refused, stop and report to the owner; PR 1 is redesigned.
- [ ] **Step 2: Account catalog.** With that token, request the model list (legacy N-B2: `supported_reasoning_levels`, `service_tiers`, `visibility`). Save one redacted sample response under the findings note (no tokens, no account ids).
- [ ] **Step 3: Claude source.** Try, and record the result of each: `claude --help`, an init event from `claude -p ... --output-format stream-json` (does it name the model or others?), and any settings or subcommand that lists models or per-model effort. Conclude with the best source, or "none, free text".
- [ ] **Step 4: Fast Mode mechanisms.** For Claude, test a `fastMode` setting via `--settings` with `--setting-sources ""` still in place. For Codex, test a `service_tier`-style `-c` key with `--strict-config` (a made-up key must fail, a real one must pass). Record the exact flag or key, or "unsupported".
- [ ] **Step 5: Commit the findings note** on a `docs/` branch (not the spike code): `git commit -m "docs: record catalog and fast mode spike findings"`.

---

## PR 1: Catalog and sign-in

Open a GitHub issue first (`gh issue create`, milestone to be chosen with the owner), then branch `issue-<n>-cli-fed-model-options`.

### Task 1: Amend the rules and the spec

**Files:**
- Modify: `AGENTS.md` (non-negotiable rule 2, and the stack table row for `ai`)
- Modify: `docs/spec.md` (§2 decision 2, §4, §6 "Sync and credentials" line)
- Modify: `docs/ai-cli.md` (add an "OpenAI account" section citing the findings note)

**Interfaces:** none.

- [ ] **Step 1:** Rewrite rule 2 so it states: Claude only through the local `claude` CLI; the OpenAI subscription account may be used through the app's own sign-in with the token only in the OS keychain; never read or copy the CLIs' credential files; never call the Anthropic API; Codex runs still use `codex exec` until the Codex-runs project is approved. Keep the read-only-tools and blind-check lines unchanged.
- [ ] **Step 2:** In `docs/spec.md` §6 narrow "ChatGPT OAuth … the model catalog" so only the legacy tool loop and legacy sign-in specifics stay out; describe the account catalog, sign-in and CLI-fed options in §2/§4.
- [ ] **Step 3:** Commit: `git commit -m "docs: allow the OpenAI account for the model catalog"`.

### Task 2: Catalog domain and cache

**Files:**
- Create: `src-tauri/src/ai/catalog.rs`
- Create: `src-tauri/src/ai/catalog/tests.rs`
- Modify: `src-tauri/src/ai/mod.rs` (declare `pub mod catalog;`)

**Interfaces:**
- Produces (all `#[derive(Serialize, TS)]` with `#[ts(export)]` and camelCase, in `catalog.rs`):
  - `ModelOption { id: String, label: String, efforts: Vec<Effort> }`
  - `CatalogStatus` enum: `Ready`, `Unavailable { reason: String }` (reason is pt-BR, safe to show)
  - `AgentCatalog { agent: Agent, status: CatalogStatus, models: Vec<ModelOption> }`
  - `Catalog { agents: Vec<AgentCatalog> }`
  - `pub trait CatalogSource: Send + Sync { fn agent(&self) -> Agent; fn load(&self) -> AgentCatalog; }`
  - `pub struct CatalogCache` with `new(sources: Vec<Box<dyn CatalogSource>>)`, `get(&self) -> Catalog` (loads once, then cached) and `refresh(&self) -> Catalog`. Managed Tauri state.

- [ ] **Step 1: Write failing tests** in `catalog/tests.rs` using a fake `CatalogSource` that counts `load` calls: `get_loads_each_source_once`, `refresh_loads_again`, `concurrent_refreshes_share_one_load` (two threads, one `load`), `unavailable_source_is_kept_in_the_result_not_an_error`.
- [ ] **Step 2:** Run `cargo test --manifest-path src-tauri/Cargo.toml catalog` → FAIL (module missing).
- [ ] **Step 3:** Implement the types and `CatalogCache` (a `Mutex<Option<Catalog>>` plus a guard so concurrent refreshes wait for the in-flight one).
- [ ] **Step 4:** Run the same command → PASS.
- [ ] **Step 5:** Commit `feat: add the model catalog cache` (include regenerated bindings).

### Task 3: Claude source

**Files:**
- Create: `src-tauri/src/ai/catalog/claude.rs`
- Test: `src-tauri/src/ai/catalog/claude_tests.rs` (or inside `catalog/tests.rs`)

**Interfaces:**
- Consumes: `CatalogSource`, `AgentCatalog`, `ModelOption` (Task 2); the Claude binary path resolution already used by `src-tauri/src/ai/discovery.rs` and the settings CLI path.
- Produces: `pub struct ClaudeSource { /* binary path */ }` implementing `CatalogSource`.

- [ ] **Step 1: Write failing tests** with a fake `claude` executable (follow the pattern in `src-tauri/src/ai/runner/tests.rs`): `claude_source_reads_models_from_the_cli` (only if Task 0 found a real source; the fake prints the shape Task 0 recorded), `claude_source_without_a_cli_is_unavailable_with_a_pt_br_reason`, `claude_source_with_unparseable_output_is_unavailable`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement per the findings note. **If Task 0 found no source,** implement `load` returning `CatalogStatus::Unavailable` with the reason "O Claude Code não informa a lista de modelos; digite o nome." and an empty model list, keeping the tests that apply.
- [ ] **Step 4:** Run → PASS.
- [ ] **Step 5:** Commit `feat: add the Claude catalog source`.

### Task 4: OpenAI sign-in

**Files:**
- Create: `src-tauri/src/ai/openai_auth.rs`
- Create: `src-tauri/src/ai/openai_auth/tests.rs`
- Modify: `src-tauri/src/ai/secrets.rs` (add `openai_token`, `set_openai_token`, `clear_openai_token` beside the RemNote ones; secret name `"openai-token"`)
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs` (`register()`)
- Modify: `src/api.ts`

**Interfaces:**
- Produces (Rust): `pub enum SignInState { SignedOut, SignedIn }` (`ts-rs`, camelCase); `pub fn sign_in(secrets: &Secrets, http: &dyn OpenAiHttp) -> AppResult<SignInState>`; `pub fn sign_out(secrets: &Secrets) -> AppResult<()>`; `pub fn state(secrets: &Secrets) -> AppResult<SignInState>`; `pub fn access_token(secrets: &Secrets, http: &dyn OpenAiHttp) -> AppResult<Option<String>>` (refreshes when expired; one failure returns `None`); `pub trait OpenAiHttp` hiding network calls so tests use a fake.
- Produces (commands): `openai_sign_in`, `openai_sign_out`, `openai_sign_in_state`, each returning `AppResult<SignInState>`; `api.openaiSignIn()`, `api.openaiSignOut()`, `api.openaiSignInState()` in `src/api.ts`. Follow the `add-command` skill for each.

- [ ] **Step 1: Write failing tests:** `sign_in_stores_the_token_in_secrets_and_never_in_settings`, `sign_out_clears_the_token`, `access_token_refreshes_when_expired`, `failed_refresh_returns_none_and_clears_nothing_twice` (no loop), `state_reflects_the_keychain`. Use `Secrets::memory()` and a fake `OpenAiHttp`.
- [ ] **Step 2:** Run `cargo test ... openai_auth` → FAIL.
- [ ] **Step 3:** Implement the flow recorded in the findings note (browser plus local redirect, or device code). Open the browser with the existing opener used by `src/pages/settings/openFolder.ts`'s backend command, or the Tauri opener plugin if already a dependency; do not add a dependency without checking `Cargo.toml` first.
- [ ] **Step 4:** Run → PASS. Then add the commands, register them, add the `api.ts` functions, run `cargo test` to regenerate bindings.
- [ ] **Step 5:** Commit `feat: add the OpenAI account sign-in`.

### Task 5: Codex account source

**Files:**
- Create: `src-tauri/src/ai/catalog/codex.rs`
- Test: `src-tauri/src/ai/catalog/tests.rs`

**Interfaces:**
- Consumes: `CatalogSource` (Task 2); `access_token` and `OpenAiHttp` (Task 4).
- Produces: `pub struct CodexSource { secrets, http }` implementing `CatalogSource`.

- [ ] **Step 1: Write failing tests** with a fake `OpenAiHttp` returning the redacted sample from the findings note: `codex_source_lists_listed_models_with_their_efforts`, `codex_source_hides_models_not_visible_in_the_list`, `codex_source_ignores_efforts_outside_the_enum` (Review Focus 1), `codex_source_signed_out_is_unavailable_with_a_pt_br_reason`, `codex_source_request_failure_is_unavailable`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement `load`: get the token, request the model list as the findings note specifies, map to `ModelOption`, drop unknown efforts.
- [ ] **Step 4:** Run → PASS.
- [ ] **Step 5:** Commit `feat: add the Codex account catalog source`.

### Task 6: Catalog command and API

**Files:**
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs` (manage `CatalogCache`, register command)
- Modify: `src/api.ts`

**Interfaces:**
- Consumes: `CatalogCache`, `Catalog` (Task 2), both sources (Tasks 3, 5).
- Produces: `commands::ai_catalog(refresh: bool) -> AppResult<Catalog>`; `api.aiCatalog(refresh?: boolean): Promise<Catalog>`.

- [ ] **Step 1:** Add a test that builds the cache with fake sources and asserts `get`/`refresh` through the same function the command calls (domain function `fn catalog(cache: &CatalogCache, refresh: bool) -> Catalog`).
- [ ] **Step 2:** Run → FAIL; implement; run → PASS.
- [ ] **Step 3:** Wrap in the command, register, add `api.aiCatalog`, run `cargo test` to regenerate bindings.
- [ ] **Step 4:** Commit `feat: expose the model catalog command`.

### Task 7: Stale-model validation in the backend

**Files:**
- Modify: `src-tauri/src/ai/settings.rs` (the function that builds a run's `JobSpec` from a `RoleConfig`, near the `config.model.clone()` call around line 269)
- Test: `src-tauri/src/ai/settings/tests.rs`

**Interfaces:**
- Consumes: `CatalogCache` (Task 2).
- Produces: running a role fails with `AppError::Invalid("O modelo “{model}” não está mais disponível. Escolha outro em Configurações.")` when the agent's catalog is `Ready` and the model is not listed; when `Unavailable`, any valid model name passes.

- [ ] **Step 1: Write failing tests:** `run_is_blocked_for_a_model_the_catalog_no_longer_lists`, `run_is_allowed_for_any_valid_model_when_the_catalog_is_unavailable`, `run_is_allowed_for_a_listed_model`.
- [ ] **Step 2:** Run → FAIL; implement; run → PASS.
- [ ] **Step 3:** Run the `spoiler-auditor` agent over the diff. Commit `feat: block runs with a model the catalog no longer lists`.

### Task 8: Frontend selectors

**Files:**
- Create: `src/pages/settings/catalog.ts` (a shared Solid resource: `useCatalog()` returning `{ catalog, loading, refresh }`, one fetch per session)
- Create: `src/components/ModelEffortFields.tsx` and `src/components/ModelEffortFields.test.tsx`
- Modify: `src/pages/settings/RolesSection.tsx`, `src/pages/generate/RolePicker.tsx`
- Modify: `src/pages/settings/labels.ts` (remove the `efforts` array if nothing else uses it; keep `effortLabels`)
- Test: `src/pages/Settings.test.tsx`, `src/pages/Generate.test.tsx`

**Interfaces:**
- Consumes: `api.aiCatalog` (Task 6), `Catalog` bindings.
- Produces: `<ModelEffortFields agent model effort onChange />` used by both pages. It renders a Model `<select>` from the catalog, an Effort `<select>` from the selected model's `efforts`, and the free-text fallback with a pt-BR warning when the agent is `Unavailable`. A saved model absent from a `Ready` catalog is shown as an extra option "«modelo» (não disponível)" and the parent is told the role is invalid so saving or generating is disabled.

- [ ] **Step 1: Write failing component tests** (mock `api.aiCatalog`): `shows_models_and_efforts_from_the_catalog`, `changing_the_model_changes_the_effort_options`, `unavailable_source_shows_a_warning_and_free_text`, `stale_saved_model_is_flagged_and_marks_the_role_invalid`, `changing_the_agent_selects_that_agents_first_model` (Review Focus 4), `models_without_efforts_use_the_other_reported_efforts` (Review Focus 2), `atualizar_refetches_once` (Review Focus 5).
- [ ] **Step 2:** Run `npm test` → FAIL.
- [ ] **Step 3:** Implement the component and wire it into both pages; add the "Atualizar" button next to the selectors in Funções.
- [ ] **Step 4:** Run `npm test`, `npm run typecheck`, `npm run lint` → PASS.
- [ ] **Step 5:** Commit `feat: feed model and effort selectors from the catalog`.

### Task 9: Sign-in section in Configurações

**Files:**
- Create: `src/pages/settings/OpenAiSection.tsx` and `src/pages/settings/OpenAiSection.test.tsx`
- Modify: `src/pages/Settings.tsx` (mount the section near "Agentes")

**Interfaces:**
- Consumes: `api.openaiSignIn`, `api.openaiSignOut`, `api.openaiSignInState` (Task 4); `showError`/`showToast` from `src/components`.

- [ ] **Step 1: Write failing tests:** `shows_signed_out_with_an_entrar_button`, `entrar_calls_sign_in_and_shows_signed_in`, `sair_calls_sign_out`, `a_sign_in_error_is_shown_in_pt_br`. Assert the page never renders a token (the API never returns one).
- [ ] **Step 2:** Run → FAIL; implement the section (pt-BR: "Conta OpenAI", "Entrar", "Sair", "Conectada"/"Desconectada"); run → PASS.
- [ ] **Step 3:** After sign-in or sign-out, call the catalog refresh so the selectors update.
- [ ] **Step 4:** Commit `feat: add the OpenAI account section to settings`.

### Task 10: Preflight and PR

- [ ] **Step 1:** Run the `preflight` skill; fix anything it reports.
- [ ] **Step 2:** Run the `spoiler-auditor` agent on the whole branch.
- [ ] **Step 3:** Open the PR with `Closes #<n>`, how it was verified (including a manual run on both computers: sign in, open Funções and Gerar, refresh, sign out), and the spec/doc updates listed.

---

## PR 2: Fast Mode (outline; detail after Task 0 and PR 1 land)

Open a second issue and branch. Tasks, each with the same test-first pattern:

1. `ModelOption` gains `supports_fast: bool` (Codex source from the account's service tiers; Claude source per Task 0).
2. `RoleConfig` gains `fast: bool` (default `false`, `#[serde(default)]` so saved roles still load); settings validation rejects `fast: true` for a model that doesn't support it when the catalog is `Ready`.
3. Runner support per agent using only the mechanism recorded in the findings note, in `src-tauri/src/ai/claude.rs` and `codex.rs`; an agent with no verified mechanism never receives the option. Fake-CLI tests assert the exact arguments or config.
4. Frontend: a "Modo rápido" checkbox in `ModelEffortFields`, shown only when the selected model supports it; included in the Gerar override.
5. `docs/ai-cli.md` updated with the verified mechanisms; preflight and PR.

## Later project (not in this plan)

Codex runs through the account in place of `codex exec`. Needs its own brainstorm and spec changes before any plan.
