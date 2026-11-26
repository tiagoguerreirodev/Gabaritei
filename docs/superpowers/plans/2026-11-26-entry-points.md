# Entry points Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every way of starting a session goes through Banco with the filters already set.
*Erros e revisões* is removed, and the old session sources and the start dialog are deleted.

**Architecture:** Shortcuts become plain links built with `bancoHref` (Banco screen plan). Once
nothing sends the `topic`, `errors` or `reviews` session sources, they are removed from
`SessionSource` in Rust. `SourceKind` keeps those values so old attempts still load and get
their titles. The `learning_overview` command and its DTOs go away with the page. Review
scheduling, the due counts and the open-attempt rule stay.

**Tech Stack:** Solid 1.9, `@solidjs/router` 0.15, Vitest; Rust, ts-rs.

**Spec:** `docs/superpowers/specs/2026-11-26-frontend-navigation-rethink-design.md` (Navigation,
Other entry points, step 4).

## Global Constraints

- Depends on the Banco backend and Banco screen plans (`bancoHref`, `presets`, `emptyFilter`).
- Resuming an open attempt still opens Resolver directly (owner decision).
- The old route `/erros-e-revisoes` redirects to `bancoHref(presets.reviews)`, so stale links
  still work.
- Attempts stored with `source_kind` `topic`, `errors` or `reviews` keep loading with their
  current titles. Never delete or rewrite them (AGENTS.md rule 4).
- The per-question error list with links to past attempts goes away along with the page (the
  approved fold). Past attempts are reached from Histórico (plan 5).
- Run `spoiler-auditor` on the Rust part (session sources).
- `npm run lint`, `npm run typecheck`, `npm test`, `cargo test`, `cargo clippy --all-targets -- -D
  warnings`, `cargo fmt --check` green after every task.

## Review Focus

- **An old *topic*, *errors* or *reviews* attempt** opens in Resolver with the same title as
  before. Test: Task 2 `old_source_kinds_still_load_with_their_titles`.
- **A bookmarked `#/erros-e-revisoes`** lands on Banco with *Revisões de hoje*, not on the 404
  page. Test: Task 1 `the_old_learning_route_redirects_to_the_reviews_preset`.
- **Início with due reviews but some held in an open attempt** still shows the "N em tentativa
  aberta" note on its card. Test: Task 1 `the_reviews_card_keeps_the_held_note`.
- **The sidebar's main menu** no longer lists the removed page, and keyboard focus still moves to
  `main` after a route change. Test: Task 1 `the_menu_has_no_learning_entry`, plus the existing
  focus test.
- **Typing a count in the old dialog's place:** there's no count field outside Banco any more,
  so no screen sends `questionCountLimits({ kind: "topic" })`. Test: Task 2 removes `Topic` from
  `CountContext`, and the compiler and tsc catch any remaining caller.

---

### Task 1: Links instead of dialogs; remove the page

**Files:**
- Modify: `src/pages/home/TodayCard.tsx`, `src/pages/Home.tsx` ("Revisões pendentes" link),
  `src/pages/performance/StrengthTable.tsx` (*Treinar*), `src/routes.ts`, `src/App.tsx` (the
  redirect), `src/components/icons.tsx` (drop the unused icon), tests: `src/pages/Home.test.tsx`,
  `src/pages/Performance.test.tsx`, `src/App.test.tsx`, `src/components/Layout.test.tsx`,
  `src/pages/headings.test.tsx`
- Delete: `src/pages/Learning.tsx`, `src/pages/Learning.test.tsx`,
  `src/pages/solve/StartSessionModal.tsx`

**Interfaces:**
- Consumes: `bancoHref(state: Partial<BancoState>): string`, `presets.reviews`, `emptyFilter`
  from `src/pages/bank/`.

Mapping:
- TodayCard `reviews` → `<A href={bancoHref(presets.reviews)}>` with *Revisar agora*, keeping
  the count and the held note.
- TodayCard `weakest` → `bancoHref({ filter: { ...emptyFilter, topicIds: [topicId] } })`,
  *Treinar este assunto*.
- TodayCard `resume` and `start` stay as they are.
- Home "Revisões pendentes" → `bancoHref(presets.reviews)`.
- StrengthTable *Treinar* → a link to `bancoHref({ filter: { ...emptyFilter, topicIds:
  [topic.topicId] } })`, keeping its `aria-label`.

- [ ] **Step 1: Update the tests first**: in `Home.test.tsx`,
  `the_reviews_card_links_to_the_reviews_preset` (href equals
  `"#/banco?status=due,wrong&ordem=dueFirst"` as the router renders it),
  `the_weakest_topic_card_links_to_banco_with_the_topic`, `the_reviews_card_keeps_the_held_note`.
  In `Performance.test.tsx`, `treinar_links_to_banco_with_the_topic`. In `App.test.tsx`,
  `the_old_learning_route_redirects_to_the_reviews_preset`. In `Layout.test.tsx`,
  `the_menu_has_no_learning_entry`. Delete the tests that open `StartSessionModal` from these
  screens.
- [ ] **Step 2: Run** `npm test` → the new tests FAIL.
- [ ] **Step 3: Implement** the links, remove the route entry, and add the redirect route
  (`{ path: "/erros-e-revisoes", component: () => <Navigate href={bancoHref(presets.reviews)
  .slice(1)} /> }` or the router's equivalent) before the `*404` route. Delete the page, its
  test, `StartSessionModal` and its CSS rules.
- [ ] **Step 4: Run** `npm run lint && npm run typecheck && npm test` → green.
- [ ] **Step 5: Commit** `feat: start every session from Banco`.

### Task 2: Remove the old session sources and the learning overview

**Files:**
- Modify: `src-tauri/src/attempts.rs` (`SessionSource`), `src-tauri/src/attempts/session.rs`,
  `src-tauri/src/question_count.rs` (`CountContext::Topic`), `src-tauri/src/learning.rs`
  (`overview`, `LearningOverview`, `LearningItem`, and helpers used only by them),
  `src-tauri/src/commands.rs` (`learning_overview`), `src-tauri/src/lib.rs` (`register()`),
  `src/api.ts`, tests in `attempts/tests.rs`, `learning/tests.rs`, `selection/tests.rs`,
  `commands.rs`
- Delete (regenerated away): `src/bindings/LearningOverview.ts`, `src/bindings/LearningItem.ts`

**Interfaces:**
- Produces: `SessionSource` = `Collection { collection_id } | Filter { filter, count, order }`.
  `SourceKind` is unchanged.

- [ ] **Step 1: Move the behavior tests first.** Every `attempts/tests.rs` and `learning/tests.rs`
  test that starts a `Topic`, `Errors` or `Reviews` session and checks *which* questions it
  picks gets an equivalent `Filter` test in `selection/tests.rs`, unless one exists already
  (`due_first_matches_the_reviews_session` covers reviews). The equivalents: topic →
  `topicIds: [id]`, `UnseenFirst`; errors → `statuses: [Wrong]` (or `[Wrong, Blank]` with
  blanks), `RecentErrorsFirst`; reviews → `[Due, Wrong]`, `DueFirst`. Write
  `old_source_kinds_still_load_with_their_titles`: insert one attempt per old kind directly with
  SQL, `load` each, and assert the titles are the topic path, *Caderno de erros* and *Revisões*.
- [ ] **Step 2: Run** `cargo test --manifest-path src-tauri/Cargo.toml` → the new tests PASS
  against the current code (they pin today's behavior before the removal).
- [ ] **Step 3: Remove** the three variants, `topic_questions` (its sort now lives in
  `selection`), `CountContext::Topic`, `learning::overview` and its DTOs, the command and
  `api.learningOverview`. Keep `review_session`/`due_questions`/`error_questions` only if
  `selection` still calls them; otherwise delete them and their tests (the `selection` tests
  cover the rules).
- [ ] **Step 4: Run** `cargo test`, clippy and fmt, then `npm run typecheck && npm test` →
  green; commit the regenerated `src/bindings/`.
- [ ] **Step 5: Spoiler audit:** run `spoiler-auditor` on the diff.
- [ ] **Step 6: Update `AGENTS.md`** (Stack and layout: no Learning page) and
  `docs/spec.md` §3.5 if plan 2's spec edit left any wording about the page. **Commit**
  `refactor: drop the old session sources`; open an issue "Entry points through Banco", branch
  `issue-<n>-entry-points`, PR `Closes #<n>`.
