# Histórico Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a *Histórico* screen that lists open and finished attempts, opens a finished one
read-only, and offers *Refazer* through Banco.

**Architecture:** A new `attempts::history` function returns one page of finished attempts, each
with its title, mode, dates, progress and a score **derived from current question state** (so key
corrections and annulments apply). Open attempts reuse `attempts::active`. The screen is a new
route that renders both lists. *Abrir* uses the existing `openAttempt`. *Refazer* is a link built
with `bancoHref`, carrying the attempt's questions (`attemptId`) or, for a caderno attempt, the
caderno.

**Tech Stack:** Rust, rusqlite, ts-rs; Solid 1.9, `@solidjs/router` 0.15, Vitest.

**Spec:** `docs/superpowers/specs/2026-11-26-frontend-navigation-rethink-design.md` (Histórico,
step 5).

## Global Constraints

- Depends on the Banco backend (`QuestionFilter.attempt_id`), the Banco screen (`bancoHref`,
  `emptyFilter`), and the foundations (design-system classes).
- **No spoilers:** a history item carries counts, score and title only. No stems, keys or
  explanations. A finished attempt shows its keys only inside Resolver, as today.
- **One home for scoring:** compute each attempt's score with `attempts::Score` and `outcome`
  from current `answer_id` and `annulled`. Never store a score.
- **No N+1 for scores:** one query loads every response of the page's attempts joined with its
  question. Titles may use `view::title` per row, as `active` does (the page holds at most
  `MAX_PAGE_SIZE` rows).
- Abandoned attempts are deleted (rule 4), so they never appear.
- Page size: `DEFAULT_PAGE_SIZE = 20`, at most `MAX_PAGE_SIZE = 100` (reuse the `bank::catalog`
  constants).
- Sidebar: *Histórico* between *Desempenho* and the waiting entries, `menu: "main"`, route
  `/historico`.
- `cargo test`, clippy, fmt, `npm run lint`, `npm run typecheck`, `npm test` green after every
  task; commit the regenerated bindings.

## Review Focus

- **A key correction after an attempt finished** changes that attempt's score in Histórico.
  Test: Task 1 `scores_follow_the_current_key`.
- **An annulled question** leaves the attempt's denominators, and an attempt where every
  question is annulled shows no percentage instead of 0%. Test: Task 1
  `an_all_annulled_attempt_has_no_achievement`, Task 2 `shows_a_dash_without_a_score`.
- **Many finished attempts:** paging keeps newest first and never repeats a row across pages
  (tie on `finished_at`). Test: Task 1 `pages_are_newest_first_without_repeats`.
- ***Refazer* on an old *topic*, *errors* or *reviews* attempt** uses that attempt's questions,
  so it still works now that those sources are gone. Test: Task 2
  `refazer_uses_the_attempt_questions_for_old_kinds`.
- **The page with no attempts at all** shows an empty state that points to Banco, not a blank
  page. Test: Task 2 `shows_an_empty_state_that_links_to_banco`.

---

### Task 1: `attempts::history`

**Files:**
- Create: `src-tauri/src/attempts/history.rs`
- Modify: `src-tauri/src/attempts.rs` (`mod history; pub use history::…`),
  `src-tauri/src/commands.rs` (`attempt_history`), `src-tauri/src/lib.rs` (`register()`),
  `src/api.ts` (`attemptHistory`)
- Test: `src-tauri/src/attempts/tests.rs`

**Interfaces:**
- Produces (TS-exported, camelCase):

```rust
pub enum HistorySource { Collection, Filter, Older }  // Older = topic, errors, reviews
pub struct HistoryQuery {
    pub source: Option<HistorySource>,
    pub mode: Option<FeedbackMode>,
    pub page: u32,
    pub page_size: u32,
}
pub struct HistoryItem {
    pub id: AttemptId,
    pub title: String,
    pub source_kind: SourceKind,
    pub collection_id: Option<CollectionId>,   // set for collection attempts (Refazer)
    pub feedback_mode: FeedbackMode,
    pub started_at: String,
    pub finished_at: String,
    pub total: u32,
    pub score: Score,
}
pub struct HistoryPage { pub items: Vec<HistoryItem>, pub total: u32, pub page: u32,
    pub page_size: u32 }
pub fn history(conn: &Connection, query: &HistoryQuery) -> AppResult<HistoryPage>;
```

  Command `attempt_history(query: HistoryQuery) -> AppResult<HistoryPage>` (async), and
  `api.attemptHistory(query)`. Also command `active_attempts() -> AppResult<Vec<ActiveAttempt>>`
  wrapping the existing `attempts::active`, and `api.activeAttempts()` (today open attempts reach
  the frontend only inside `HomeSummary`).

- [ ] **Step 1: Write the failing tests** in `attempts/tests.rs`:
  `history_lists_finished_attempts_newest_first`, `history_skips_unfinished_attempts`,
  `scores_follow_the_current_key` (finish an attempt, correct the key with the existing
  re-check apply path or a direct `UPDATE`, and the item's score changes),
  `an_all_annulled_attempt_has_no_achievement` (`achievement == None`),
  `history_filters_by_source_and_mode`, `pages_are_newest_first_without_repeats` (5 attempts
  with two equal `finished_at`, page size 2 → 3 pages, 5 distinct IDs; order by
  `finished_at DESC, id`), `a_page_size_above_100_is_refused` (`"Escolha até 100 itens por
  página."`), `history_items_have_no_question_text` (serialize; assert no stem, option or
  explanation strings).
- [ ] **Step 2: Run** `cargo test --manifest-path src-tauri/Cargo.toml history` → FAIL.
- [ ] **Step 3: Implement**: one query for the page's attempt rows (with `COUNT(*) OVER ()` for
  `total`), one query for their responses joined with `questions (answer_id, annulled)`, and the
  score folded in Rust with `outcome` + `Score::of`.
- [ ] **Step 4: Run** `cargo test`, clippy, fmt → green; commit bindings.
- [ ] **Step 5: Spoiler audit** with `spoiler-auditor`. **Commit**
  `feat: list finished attempts with derived scores`.

### Task 2: The Histórico screen

**Files:**
- Create: `src/pages/History.tsx`, `src/pages/History.test.tsx`, `src/styles/history.css`
- Modify: `src/routes.ts`, `src/components/icons.tsx` (a history icon in the line set: 24px grid,
  `stroke-width: 1.75`, round caps, `currentColor`), `src/styles.css`,
  `src/components/Layout.test.tsx`, `src/pages/headings.test.tsx`, `src/labels.ts`

**Interfaces:**
- Consumes: `api.activeAttempts()`, `api.attemptHistory` (Task 1), `openAttempt(id)`, `bancoHref`, `emptyFilter`, `formatDateTime`,
  `formatPercent`, `modeLabel`.

Layout (design doc sketch): `<h1>Histórico</h1>`; filters *Origem* (Todas, Cadernos, Filtros do
Banco, Anteriores) and *Modo* (Estudo e Simulado, Estudo, Simulado) as `.field` selects; section
*Em andamento* (each row: title, mode, "8/20 respondidas", started date, *Continuar*); section
*Finalizadas* (title, mode, "34/40", achievement `formatPercent`, finished date, *Abrir*,
*Refazer*); a pager (*Anterior* / *Próxima*) when `total > pageSize`. Rows are a list inside a
`card`, with buttons `btn` and the *Continuar* one `btn btn-primary`.

*Refazer* link: collection attempts → `bancoHref({ filter: { ...emptyFilter, collectionId } })`;
every other kind → `bancoHref({ filter: { ...emptyFilter, attemptId: id } })`.

- [ ] **Step 1: Write the failing tests** with `mockIPC`: `lists_open_attempts_first`,
  `opens_a_finished_attempt` (click *Abrir* → `location.hash` is `#/resolver?tentativa=<id>`),
  `refazer_a_caderno_attempt_links_to_the_caderno`,
  `refazer_uses_the_attempt_questions_for_old_kinds`, `filters_send_the_query`
  (choosing *Simulado* sends `mode: "exam"`, page 0), `pages_through_finished_attempts`,
  `shows_a_dash_without_a_score`, `shows_an_empty_state_that_links_to_banco`
  ("Nenhuma tentativa ainda." with a link *Ir para o Banco*), `shows_a_retry_when_loading_fails`.
  In `Layout.test.tsx`, `the_menu_lists_historico_after_desempenho`.
- [ ] **Step 2: Run** `npm test` → FAIL.
- [ ] **Step 3: Implement** the route, page, icon and styles.
- [ ] **Step 4: Run** `npm run lint && npm run typecheck && npm test` → green.
- [ ] **Step 5: Check by hand** (`npm run tauri dev`): finish an attempt, see it listed, *Abrir*,
  *Refazer*, Back.
- [ ] **Step 6: Update** `AGENTS.md` (Stack and layout: `attempts/history.rs`, `pages/History`)
  and `docs/spec.md` (the Histórico section, if plan 2's edit left it as a stub). **Commit**
  `feat: add the Histórico screen`; open an issue "Histórico", branch `issue-<n>-historico`, PR
  `Closes #<n>`.
