# Banco backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the backend count and start sessions from a combined filter (topics × status ×
caderno × past attempt), so Banco can replace the tree, the topic session, and the errors and
reviews sessions.

**Architecture:** A new domain module `selection` owns the filter, its counts (with facets) and
the question pick for a session. It reuses the existing rules from `learning` (latest result,
the open-attempt rule, the review order) instead of redefining them. `attempts::start` gains a
`Filter` source. Migration `0003` lets `attempts.source_kind` be `'filter'`, and the migration
runner turns foreign keys off around a migration so that a table rebuild can't cascade-delete
responses.

**Tech Stack:** Rust (Tauri 2), rusqlite, ts-rs, serde.

**Spec:** `docs/superpowers/specs/2026-11-26-frontend-navigation-rethink-design.md` (Banco,
Backend changes, step 2). Spec sections §3.3–§3.5 of `docs/spec.md` hold the existing rules this
reuses.

## Global Constraints

- **No spoilers:** `SelectionSummary` carries counts and IDs only: no stems, options, keys or
  explanations.
- **Append-only migrations:** add `0003_filter_attempts.sql`; never edit `0001`/`0002`. Tables
  stay `STRICT`.
- **Thin commands:** `selection_summary` decodes, calls `selection::summary`, encodes.
- **Dependency injection:** `now: &str` and `rng: &mut impl Rng` are parameters; no clock or
  global RNG in domain code.
- **No N+1:** a summary is a fixed number of queries whatever the filter size.
- **Exhaustive matches** on `StudyStatus` and `SessionOrder` (no `_ =>`).
- Status rules are the existing ones: *wrong* and *blank* are `learning`'s latest-result rule (a
  later blank never hides an earlier wrong answer; finished attempts only; annulled excluded);
  *due* is `review_state.due_at <= now`; the open-attempt rule is `learning`'s
  `OPEN_ATTEMPT_QUESTIONS_SQL`.
- Session size: 1 to `MAX_SELECTION_SESSION = 200`; Banco's default count 20.
- pt-BR error messages, English code.
- Commit the regenerated `src/bindings/`.
- `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` green after every
  task (all with `--manifest-path src-tauri/Cargo.toml`).

## Review Focus

- **Upgrading a real database with attempts and responses:** after `0003`, every attempt and every
  response is still there and foreign keys still hold. Test: Task 1
  `migration_3_keeps_attempts_and_responses`.
- **An empty filter** (nothing checked) means the whole bank minus annulled questions, not an
  empty set. Test: Task 2 `an_empty_filter_matches_every_valid_question`.
- **A question under two checked topics** counts once. Test: Task 2
  `a_question_under_two_topics_counts_once`.
- **Order not allowed for the statuses** (*Revisões primeiro* without *due*) is refused with a
  pt-BR message, never silently changed. Test: Task 3 `due_first_needs_the_due_status`.
- **Every match is in an open attempt:** starting is refused with the existing "no questions"
  message, not an empty attempt. Test: Task 3 `a_filter_whose_questions_are_all_held_is_refused`.

---

### Task 1: Migration `0003` and a foreign-key-safe runner

**Files:**
- Create: `src-tauri/migrations/0003_filter_attempts.sql`
- Modify: `src-tauri/src/db.rs` (`MIGRATIONS`, `migrate`), `src-tauri/src/attempts.rs`
  (`SourceKind`, `SourceRef`), `src-tauri/src/attempts/view.rs` (`title`)
- Test: `src-tauri/src/db.rs` tests

**Interfaces:**
- Produces: `SourceKind::Filter` stored as `"filter"`; `SourceRef::Filter(QuestionFilter)` stored
  as the filter's JSON in `attempts.source_ref` (`QuestionFilter` comes from Task 2; in this task
  add only the `SourceKind` variant and the schema, then wire `SourceRef::Filter` in Task 3).

- [ ] **Step 1: Write the failing test** `migration_3_keeps_attempts_and_responses` in `db.rs`
  tests: open a connection, run `migrate(conn, &MIGRATIONS[..2])`, insert one topic, one
  question, one finished attempt with `source_kind = 'topic'` and two responses; then run
  `migrate(conn, MIGRATIONS)`. Assert `user_version == 3`, both responses are still there,
  `PRAGMA foreign_key_check` returns no rows, `PRAGMA foreign_keys` is `1` again, and inserting an
  attempt with `source_kind = 'filter'` succeeds.
- [ ] **Step 2: Run it**: `cargo test --manifest-path src-tauri/Cargo.toml migration_3` →
  FAIL (no migration 3).
- [ ] **Step 3: Write `0003_filter_attempts.sql`** with the SQLite table-rebuild procedure: create
  `attempts_new` with the same columns as `0001`'s `attempts` but
  `CHECK (source_kind IN ('collection', 'topic', 'errors', 'reviews', 'filter'))` and
  `source_ref` documented as "collection id, topic id, or the filter as JSON"; copy every row;
  `DROP TABLE attempts`; `ALTER TABLE attempts_new RENAME TO attempts`; recreate
  `attempts_by_finished_at`. Append it to `MIGRATIONS`.
- [ ] **Step 4: Make `migrate` foreign-key-safe.** Before the loop: `PRAGMA foreign_keys = OFF`
  (it can't change inside a transaction). In each migration's transaction, after the batch, run
  `PRAGMA foreign_key_check` and return `AppError` if it reports a row (the transaction rolls
  back). After the loop, and on every error path, set `foreign_keys = ON` again. Comment why:
  with foreign keys on, `DROP TABLE` deletes rows and cascades to `responses`.
- [ ] **Step 5: Add `SourceKind::Filter` (`"filter"`)** to the enum and `stored_enum!`, and
  handle it in every `match` the compiler points at (`SourceRef::from_storage` returns `None` for
  now; `title` returns `None`). Run `cargo test --manifest-path src-tauri/Cargo.toml` → PASS,
  including the schema comparison tests in `backup` (update their expected schema if they pin
  the `CHECK` text).
- [ ] **Step 6: Commit** `feat: allow filter attempts in the schema`.

### Task 2: `selection` module: filter, statuses and summary

**Files:**
- Create: `src-tauri/src/selection.rs`, `src-tauri/src/selection/tests.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod selection;`), `src-tauri/src/learning.rs` (widen
  `Latest`, `latest_results`, `OPEN_ATTEMPT_QUESTIONS_SQL`, `open_attempt_questions` to
  `pub(crate)`; no behavior change), `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`
  (`register()`), `src/api.ts`

**Interfaces:**
- Produces (all `#[derive(TS)] #[ts(export)]`, camelCase):

```rust
pub struct QuestionFilter {            // Deserialize + Serialize, Default
    pub topic_ids: Vec<TopicId>,       // empty = every topic
    pub statuses: Vec<StudyStatus>,    // empty = any status; OR inside, AND with the rest
    pub collection_id: Option<CollectionId>,
    pub attempt_id: Option<AttemptId>, // "the questions of this attempt" (Refazer)
}
pub enum StudyStatus { Unseen, Wrong, Blank, Due, Flagged }   // lowercase on the wire
pub struct SelectionSummary {
    pub matching: u32,                 // annulled never match
    pub in_open_attempt: u32,          // of `matching`, held by an unfinished attempt
    pub by_area: Vec<AreaCount>,       // { area: String, questions: u32 }, Portuguese order
    pub by_status: Vec<StatusCount>,   // { status: StudyStatus, questions: u32 }, enum order
    pub by_topic: Vec<TopicCount>,     // { topic_id: TopicId, questions: u32 }
    pub allowed_orders: Vec<SessionOrder>, // what Banco may offer for these statuses
}
pub enum SessionOrder { UnseenFirst, Random, DueFirst, RecentErrorsFirst } // camelCase wire
impl SessionOrder { pub fn allowed_for(statuses: &[StudyStatus]) -> Vec<SessionOrder>; }
// UnseenFirst and Random always; DueFirst when Due is in statuses; RecentErrorsFirst when
// Wrong or Blank is. Task 3's `pick` refuses an order not in this list.
pub fn summary(conn: &Connection, filter: &QuestionFilter, now: &str)
    -> AppResult<SelectionSummary>;
pub(crate) fn matching_questions(conn: &Connection, filter: &QuestionFilter, now: &str)
    -> AppResult<Vec<Candidate>>;      // used by Task 3
```

  Command `selection_summary(filter: QuestionFilter) -> AppResult<SelectionSummary>` (async,
  `now` from `clock::now()`), and `api.selectionSummary(filter)` in `src/api.ts`.
- Facets: `by_status` counts ignore the status filter (each status's count under the other
  filters), and `by_topic` ignores the topic filter. That lets Banco show what each checkbox
  would add. `by_area` follows the full filter.

Status definitions (one place, `StudyStatus::holds(&self, facts: &QuestionFacts) -> bool`, where
`QuestionFacts { ever_answered: bool, latest: Option<LatestStatus>, due: bool, flagged: bool }`):
- `Unseen`: no response with a choice in any attempt.
- `Wrong` / `Blank`: `learning`'s latest result is wrong / blank.
- `Due`: a `review_state` row with `due_at <= now`.
- `Flagged`: flagged in the response from the question's most recently started attempt.

- [ ] **Step 1: Write the failing tests** in `selection/tests.rs`, each on a fresh in-memory
  database with a small synthetic bank (reuse `bank::question::tests::{multiple, boolean}` and
  `import_questions`, as `attempts/tests.rs` does), and fixed `now`:
  - `an_empty_filter_matches_every_valid_question`: 3 questions, 1 annulled → `matching == 2`.
  - `topics_combine_with_or`: questions in topics A, B, C; filter `[A, B]` → 2.
  - `a_question_under_two_topics_counts_once`: one question in A and B; filter `[A, B]` → 1.
  - `statuses_combine_with_or_and_topics_with_and`: in topic A one wrong and one unseen, in B
    one wrong; filter topics `[A]`, statuses `[Wrong, Unseen]` → 2; statuses `[Wrong]` → 1.
  - `a_later_blank_keeps_an_earlier_wrong_answer_wrong`: wrong then blank in finished attempts →
    matches `Wrong`, not `Blank`.
  - `due_uses_now`: a review due tomorrow does not match `Due` today, and matches tomorrow.
  - `flagged_reads_the_latest_attempt`: flagged in an old attempt, unflagged in a newer one → no
    match.
  - `collection_and_attempt_limit_the_set`: a collection of 2 out of 3 questions → 2; an attempt
    holding 1 → 1.
  - `counts_held_questions_separately`: 2 matching, 1 in an open attempt →
    `matching 2, in_open_attempt 1`.
  - `status_counts_ignore_the_status_filter` and `topic_counts_ignore_the_topic_filter`, each
    checking the whole `Vec` with `assert_eq!`.
  - `orders_follow_the_statuses`: `[]` → `[UnseenFirst, Random]`; `[Due]` adds `DueFirst`;
    `[Blank]` adds `RecentErrorsFirst`.
  - `the_summary_has_no_question_text`: serialize a summary to JSON; assert it contains none of
    the fixture's stem, option text or explanation strings.
- [ ] **Step 2: Run**: `cargo test --manifest-path src-tauri/Cargo.toml selection` → FAIL
  (module missing).
- [ ] **Step 3: Implement.** Load facts for every candidate question in a fixed set of queries:
  candidates (non-annulled, joined with `question_topics` / `collection_items` / `responses` of
  the attempt as the filter asks, `DISTINCT`), `latest_results`, answered-ever, due set, latest
  flag, open-attempt set, and topic and area per question. Then filter and count in Rust. Order
  `by_area` with `ordering::fold`, as `catalog` does.
- [ ] **Step 4: Add the command, register it, add `api.selectionSummary`**, run
  `cargo test --manifest-path src-tauri/Cargo.toml` (regenerates `src/bindings/`) → PASS; then
  `npm run typecheck`.
- [ ] **Step 5: Commit** `feat: count questions by a combined filter`.

### Task 3: Start a session from a filter

**Files:**
- Modify: `src-tauri/src/attempts.rs` (`SessionSource`, `SourceRef`, `MAX_SELECTION_SESSION`),
  `src-tauri/src/attempts/session.rs`, `src-tauri/src/attempts/view.rs` (`title`),
  `src-tauri/src/selection.rs`, `src-tauri/src/question_count.rs`, `src-tauri/src/learning.rs`
  (widen `fill_review_session` to `pub(crate)`)
- Test: `src-tauri/src/selection/tests.rs`, `src-tauri/src/attempts/tests.rs`,
  `src-tauri/src/question_count.rs` tests

**Interfaces:**
- Consumes: `QuestionFilter`, `SessionOrder`, `matching_questions` (Task 2);
  `SourceKind::Filter` (Task 1).
- Produces:

```rust
SessionSource::Filter { filter: QuestionFilter, count: u32, order: SessionOrder }
pub(crate) fn pick(conn: &Connection, filter: &QuestionFilter, count: u32,
    order: SessionOrder, now: &str, rng: &mut impl Rng) -> AppResult<Vec<QuestionId>>;
SourceRef::Filter(QuestionFilter)           // stored as JSON in attempts.source_ref
CountContext::Selection { available: u32 }  // min 1, max available.min(200), initial 20.min(max)
```

  The existing `Collection`, `Topic`, `Errors` and `Reviews` sources stay for now (plan 4
  removes the last three once nothing sends them).

Order rules (in `pick`, after leaving out held questions):
- `UnseenFirst`: never answered first, then least recently answered, ties random (the
  `topic_questions` rule; move that sort into `selection` and have `topic_questions` call it).
- `Random`: shuffled.
- `DueFirst`: needs `StudyStatus::Due` in the filter. Due questions most overdue first, then the
  rest by most recent error, with `learning::fill_review_session`. With statuses `[Due, Wrong]`
  this equals today's reviews session.
- `RecentErrorsFirst`: needs `Wrong` or `Blank`. Latest result time, newest first (the error
  notebook order).

- [ ] **Step 1: Write the failing tests**:
  - `selection`: `unseen_first_puts_never_answered_first`, `due_first_needs_the_due_status`
    (`AppError::Invalid("A ordem Revisões primeiro exige o status Revisão vencida.")`),
    `recent_errors_first_needs_an_error_status` (`"A ordem Mais recentes primeiro exige o
    status Erradas ou Em branco."`), `due_first_matches_the_reviews_session` (the same IDs, in
    the same order, as `learning::review_session` for statuses `[Due, Wrong]`),
    `held_questions_are_left_out`, `count_caps_the_pick`.
  - `attempts/tests.rs`: `a_filter_session_stores_its_filter` (start, reload with `load`;
    `source_kind == Filter`, `source_ref == Some(SourceRef::Filter(filter))`),
    `a_filter_session_is_titled_from_its_filter` (see the title rule below),
    `a_filter_whose_questions_are_all_held_is_refused` (the existing "Não há questões
    disponíveis para esta sessão." message), `a_filter_count_outside_one_to_200_is_refused`
    (`"Escolha de 1 a 200 questões."`).
  - `question_count.rs`: `selection_defaults_to_twenty_within_what_is_available` for
    `(0,0,0)`, `(4,4,4)`, `(250,200,20)`.
- [ ] **Step 2: Run**: `cargo test --manifest-path src-tauri/Cargo.toml` → the new tests FAIL.
- [ ] **Step 3: Implement** `pick` (checking `SessionOrder::allowed_for` first), the `Filter` arm in `session::start`,
  `SourceRef::Filter` (to and from storage as JSON; an unreadable stored filter gives `None`, as
  an unknown topic ID does), `CountContext::Selection`, and the title rule in
  `view::filter_title(conn, &QuestionFilter) -> AppResult<String>`:
  - with `attempt_id`: `"Refazer: <that attempt's title>"`;
  - else with topics: the first topic's name (Portuguese order), plus `" + N"` for the others;
  - else with a collection: its title;
  - else `"Banco"`;
  - then, when statuses are set, `" · "` plus their pt-BR labels joined by `", "`
    (`Nunca resolvidas`, `Erradas`, `Em branco`, `Revisão vencida`, `Sinalizadas`).
- [ ] **Step 4: Run** `cargo test`, `cargo clippy --all-targets -- -D warnings`,
  `cargo fmt --check` (all with `--manifest-path src-tauri/Cargo.toml`) → green; commit the
  regenerated bindings.
- [ ] **Step 5: Spoiler audit:** run the `spoiler-auditor` agent on the branch diff; fix
  anything it finds.
- [ ] **Step 6: Commit** `feat: start a session from a combined filter`.

### Task 4: Spec update and PR

**Files:**
- Modify: `docs/spec.md` (§3.1, §3.3, §3.4 "Other views", §3.5, new §3.x Histórico, as listed in
  the design doc's "Spec changes"), `AGENTS.md` (Stack and layout: `src-tauri/src/selection.rs`)

- [ ] **Step 1: Edit the spec** with the owner-approved text from the design doc. Keep the rule
  wording of §3.4/§3.5 (blank rule, ladder, open-attempt rule) unchanged; only where questions
  are chosen moves.
- [ ] **Step 2: Run the `preflight` skill** → all checks green.
- [ ] **Step 3: Commit** `docs: record filter sessions in the spec`; open an issue "Banco
  backend: filter sessions" linked to the design doc; branch `issue-<n>-banco-backend`; PR with
  `Closes #<n>`.
