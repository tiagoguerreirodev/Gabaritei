# CLI-fed Model, Effort and Fast Mode options: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Model, Effort and Fast Mode choices in "Funções" and "Gerar" come from the agents' real sources (OpenAI account for Codex, the `claude` CLI for Claude) instead of hard-coded values.

**Architecture:** A Rust `ai::catalog` module asks one `CatalogSource` per agent for its models (each with the efforts it supports), caches the answer in memory for the app session, and exposes it through one Tauri command. The frontend renders selectors from that command and falls back to free text when a source is unavailable. Codex's source runs `codex debug models` and reads only the fields it needs.

**Tech Stack:** Rust (Tauri 2, `ureq`, `keyring`, `ts-rs`), Solid.js + TypeScript, Vitest.

**Spec:** `docs/superpowers/specs/2026-11-21-cli-fed-model-options-design.md`

## Global Constraints

- UI text is pt-BR only; code, comments, docs, commits and PRs are English.
- Commands are thin; domain logic is plain Rust tested without Tauri. Only `src/api.ts` calls `invoke`. After Rust type changes run `cargo test --manifest-path src-tauri/Cargo.toml` and commit the regenerated `src/bindings/` (never hand-edit them).
- Migrations are append-only. This work is expected to need none (role config is JSON in `settings`).
- No real AI, network or keychain calls in tests: use fake sources, fake CLI executables and `Secrets::memory()`.
- Never log prompts, study content or credentials; log with `log::error(..)` only. The app reads no credential files and calls no OpenAI API (rule 2 is unchanged).
- One issue per branch and PR, branch name `issue-<n>-<slug>`, Conventional Commits in English, `Closes #<n>`.
- Spec changes made here are updated in the same PR (AGENTS.md rule 7). Rule 2 is **not** amended (the sign-in was refused; see the design's Amendment).
- Before each PR: `cargo test`, `cargo clippy ... -D warnings`, `cargo fmt --check`, `npm run lint`, `npm run typecheck`, `npm test` (use the `preflight` skill). A change touching the AI runner or settings also gets the `spoiler-auditor` agent.

## Review Focus

1. **Effort name outside the `Effort` enum** (`low|medium|high|xhigh|max|ultra`) reported by `codex debug models`: ignored, never a crash or a broken selector.
2. **Model listed with no efforts:** its Effort selector offers the enum values the source still reports for other models, or free text if none.
3. **`codex` missing, not signed in, hanging, or printing invalid or huge JSON:** the Codex source becomes `Unavailable` with a pt-BR reason and free text; the call has a timeout and an output cap.
4. **Switching a role's agent:** the model becomes the new agent's first listed model (or stays free text), never the old agent's model name.
5. **Two refreshes at once** (both pages open, or a double click on "Atualizar"): one request in flight, both callers get the same result.

---

## Step 0: Spike (throwaway, not merged)

### Task 0: Verify the four unknowns — DONE (see the findings note; sign-in refused, Codex CLI chosen)

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

## PR 1: Catalog

Open a GitHub issue first (`gh issue create`, milestone to be chosen with the owner), then branch `issue-<n>-cli-fed-model-options`.

### Task 1: Update the spec and docs

**Files:**
- Modify: `docs/spec.md` (§2 decision 2, §4, §6 "Sync and credentials" line: narrow "the model catalog" so only the legacy HTTP catalog stays out; describe CLI-fed Model/Effort options and Fast Mode)
- Modify: `docs/ai-cli.md` (add the verified `codex debug models` fields and the findings for Claude and Codex Fast Mode, citing `docs/superpowers/specs/2026-11-21-catalog-spike-findings.md`)

**Interfaces:** none. `AGENTS.md` is unchanged.

- [ ] **Step 1:** Make the edits above. State in `docs/ai-cli.md` that the Codex Fast Mode effect is unobserved.
- [ ] **Step 2:** Commit: `git commit -m "docs: describe the CLI-fed model catalog"`.

### Task 2: Catalog domain and cache

**Files:**
- Create: `src-tauri/src/ai/catalog.rs`
- Create: `src-tauri/src/ai/catalog/tests.rs`
- Modify: `src-tauri/src/ai/mod.rs` (declare `pub mod catalog;`)
- Modify: `src-tauri/src/ai/agent.rs` (add `Effort::Ultra`, name `"ultra"`, `as_str`; Codex only)
- Modify: `src/pages/settings/labels.ts` (`effortLabels.ultra = "Ultra"`)

**Interfaces:**
- Produces (all `#[derive(Serialize, TS)]` with `#[ts(export)]` and camelCase, in `catalog.rs`):
  - `ModelOption { id: String, label: String, efforts: Vec<Effort> }`
  - `CatalogStatus` enum: `Ready`, `Unavailable { reason: String }` (reason is pt-BR, safe to show)
  - `AgentCatalog { agent: Agent, status: CatalogStatus, models: Vec<ModelOption> }`
  - `Catalog { agents: Vec<AgentCatalog> }`
  - `pub trait CatalogSource: Send + Sync { fn agent(&self) -> Agent; fn load(&self) -> AgentCatalog; }`
  - `pub struct CatalogCache` with `new(sources: Vec<Box<dyn CatalogSource>>)`, `get(&self) -> Catalog` (loads once, then cached) and `refresh(&self) -> Catalog`. Managed Tauri state.

- [ ] **Step 1: Write failing tests** in `catalog/tests.rs` using a fake `CatalogSource` that counts `load` calls: `get_loads_each_source_once`, `refresh_loads_again`, `concurrent_refreshes_share_one_load` (two threads, one `load`), `unavailable_source_is_kept_in_the_result_not_an_error`, and in `agent.rs` tests `ultra_serializes_as_ultra`; also a Claude-argument test that a role with `Effort::Ultra` and agent Claude is rejected with a pt-BR message before spawning (Claude's `--effort` stops at `max`).
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

- [ ] **Step 1: Write failing tests** with a fake `claude` executable (follow the pattern in `src-tauri/src/ai/runner/tests.rs`): `claude_source_reads_models_from_the_cli` (skip: Task 0 found no listing source), `claude_source_without_a_cli_is_unavailable_with_a_pt_br_reason`, `claude_source_with_unparseable_output_is_unavailable`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement per the findings note. Task 0 found no source, so implement `load` returning `CatalogStatus::Unavailable` with the reason "O Claude Code não informa a lista de modelos; digite o nome." and an empty model list, keeping the tests that apply.
- [ ] **Step 4:** Run → PASS.
- [ ] **Step 5:** Commit `feat: add the Claude catalog source`.

### Task 4: REMOVED

The app's own OpenAI sign-in was refused by the service (spike findings). No sign-in, token storage or commands are built.

### Task 5: Codex source (`codex debug models`)

**Files:**
- Create: `src-tauri/src/ai/catalog/codex.rs`
- Test: `src-tauri/src/ai/catalog/tests.rs`

**Interfaces:**
- Consumes: `CatalogSource` (Task 2); the Codex binary path from the AI settings (see `src-tauri/src/ai/discovery.rs`); the fake-CLI helper pattern in `src-tauri/src/ai/runner/tests.rs`.
- Produces: `pub struct CodexSource { /* binary path */ }` implementing `CatalogSource`, and `pub(super) fn parse_models(json: &str) -> Result<Vec<ModelOption>, String>`.

- [ ] **Step 1: Write failing tests** using a fake `codex` executable that prints a trimmed copy of the real shape (`{"models":[{"slug":..,"display_name":..,"visibility":..,"supported_reasoning_levels":[{"effort":..}],"service_tiers":[{"id":"priority"}], ... extra large fields ...}]}`): `parse_keeps_only_listed_models`, `parse_maps_efforts_and_drops_unknown_ones` (an `"effort":"turbo"` entry is dropped, `ultra` is kept), `parse_ignores_unrelated_large_fields`, `source_runs_debug_models_with_a_timeout`, `missing_cli_is_unavailable_with_a_pt_br_reason`, `invalid_json_is_unavailable`, `oversized_output_is_unavailable` (cap the read, e.g. 8 MiB).
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement: run `codex debug models` without a shell (no `--strict-config`, not supported by `debug`), read at most the cap, parse with `serde_json` into a minimal struct, keep `visibility == "list"`, map efforts through `Effort`, label from `display_name`.
- [ ] **Step 4:** Run → PASS.
- [ ] **Step 5:** Commit `feat: add the Codex catalog source`.

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

### Task 9: REMOVED

No OpenAI sign-in section (see Task 4).

### Task 10: Preflight and PR

- [ ] **Step 1:** Run the `preflight` skill; fix anything it reports.
- [ ] **Step 2:** Run the `spoiler-auditor` agent on the whole branch.
- [ ] **Step 3:** Open the PR with `Closes #<n>`, how it was verified (including a manual run on both computers: open Funções and Gerar, refresh, change agent and model), and the spec/doc updates listed.

---

## PR 2: Fast Mode (outline; detail after Task 0 and PR 1 land)

Open a second issue and branch. Tasks, each with the same test-first pattern:

1. `ModelOption` gains `supports_fast: bool`. Codex: true when `service_tiers` is non-empty. Claude: not listed, so the checkbox is always shown for Claude.
2. `RoleConfig` gains `fast: bool` (default `false`, `#[serde(default)]` so saved roles still load); settings validation rejects `fast: true` for a model that doesn't support it when the catalog is `Ready`.
3. Runner support: Claude `--settings '{"fastMode":true}'` (added to the isolated argument list in `claude.rs`), Codex `-c service_tier="priority"` (in `codex.rs`), the mechanisms recorded in the findings note, in `src-tauri/src/ai/claude.rs` and `codex.rs`; an agent with no verified mechanism never receives the option. Fake-CLI tests assert the exact arguments or config.
4. Frontend: a "Modo rápido" checkbox in `ModelEffortFields`, shown only when the selected model supports it; included in the Gerar override.
5. `docs/ai-cli.md` updated with the verified mechanisms; preflight and PR.

## Later project

Dropped: running Codex through the account needed the sign-in that the service refused. Reopen only if the owner asks.
