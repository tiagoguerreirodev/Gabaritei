# Banco screen Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild Banco as the one place to choose questions: filters on the left, live counts
and the session settings on the right, presets on top, all state in the URL. It replaces the
*Assuntos* tree and the *Cadernos* tab.

**Architecture:** `Bank.tsx` reads its state from the URL query (`useSearchParams`), calls
`api.selectionSummary` whenever the filter changes, and renders four small components. A pure
module `filterUrl.ts` converts between the URL and the wire types. Every rule (matching, counts,
allowed orders, limits) comes from Rust (plan `2026-11-26-banco-backend.md`); the screen only
renders it. It's built with the design-system classes from the foundations plan.

**Tech Stack:** Solid 1.9, `@solidjs/router` 0.15 (`HashRouter`, `useSearchParams`), Vitest +
`@solidjs/testing-library` + `mockIPC`.

**Spec:** `docs/superpowers/specs/2026-11-26-frontend-navigation-rethink-design.md` (Banco, URL
state, A caderno on its own, step 3).

## Global Constraints

- Depends on the foundations plan (design-system classes) and the Banco backend plan
  (`QuestionFilter`, `StudyStatus`, `SessionOrder`, `SelectionSummary`, `selectionSummary`,
  `SessionSource` `filter` variant, `CountContext` `selection`).
- **Counts only, never stems** (owner decision). No question text on this screen.
- Business rules stay in Rust: the screen offers only `summary.allowedOrders`, limits come from
  `api.questionCountLimits({ kind: "selection", available })`, and errors are shown as returned.
- The URL holds IDs and wire enum names only: `#/banco?assuntos=<id,…>&status=<s,…>&caderno=<id>
  &tentativa=<id>&ordem=<order>&modo=<study|exam>`. Unknown values are dropped when parsed.
- pt-BR copy following `docs/design-system/README.md` › Content: sentence case, verbs on
  buttons, no exclamation marks, counts as "12 questões".
- Status labels: `unseen` *Nunca resolvidas*, `wrong` *Erradas*, `blank` *Em branco*, `due`
  *Revisão vencida*, `flagged` *Sinalizadas*. Order labels: `unseenFirst` *Não vistas primeiro*,
  `random` *Aleatória*, `dueFirst` *Revisões primeiro*, `recentErrorsFirst` *Mais recentes
  primeiro*. Both go in `src/labels.ts`.
- Groups combine with AND, choices inside a group with OR (the backend applies this; the copy
  must not promise anything else).
- `npm run lint`, `npm run typecheck`, `npm test` green after every task.

## Review Focus

- **Back button after starting a session:** returning from Resolver restores the same filters.
  Test: Task 1 `round_trips_every_field` plus Task 4
  `restores_filters_from_the_url`.
- **A hand-edited or stale URL** (deleted topic ID, unknown status) shows Banco with what is
  still valid, never an error page. Test: Task 1 `drops_unknown_values`.
- **Rapid checkbox clicks:** only the latest summary is rendered, so an older reply arriving late
  can't overwrite it. Test: Task 4 `shows_the_latest_summary_when_replies_arrive_out_of_order`.
- **Everything matching is held by open attempts:** *Resolver estas* is disabled, with the
  reason. Test: Task 3 `disables_start_when_nothing_is_available`.
- **The whole caderno with an open attempt:** the button reads *Continuar* and opens that
  attempt instead of failing on the backend's "already in progress" error. Test: Task 3
  `continues_the_open_attempt_of_a_whole_caderno`.

---

### Task 1: URL state (`filterUrl.ts`)

**Files:**
- Create: `src/pages/bank/filterUrl.ts`, `src/pages/bank/filterUrl.test.ts`

**Interfaces:**
- Produces:

```ts
export interface BancoState {
  filter: QuestionFilter;              // from src/bindings
  order: SessionOrder | null;          // null = backend default (first allowed)
  mode: FeedbackMode;                  // default "study"
}
export function parseBanco(params: Record<string, string | undefined>): BancoState;
export function bancoSearch(state: BancoState): Record<string, string | undefined>;
export function bancoHref(state: Partial<BancoState>): string;   // "#/banco?…", used by plan 4
export const emptyFilter: QuestionFilter;
```

- [ ] **Step 1: Write the failing tests**: `round_trips_every_field` (a full state → search →
  parse equals the state), `drops_unknown_values` (`status=wrong,bogus` → `["wrong"]`;
  `assuntos=3,x` → `[3]`; `ordem=sideways` → `null`; `modo=race` → `"study"`),
  `omits_empty_fields` (`bancoSearch` of the empty state has every key `undefined`),
  `builds_a_preset_link` (`bancoHref({ filter: { ...emptyFilter, statuses: ["due", "wrong"] },
  order: "dueFirst" })` is `"#/banco?status=due,wrong&ordem=dueFirst"`).
- [ ] **Step 2: Run** `npx vitest run src/pages/bank/filterUrl.test.ts` → FAIL.
- [ ] **Step 3: Implement**; key order in `bancoHref` is `assuntos, status, caderno, tentativa,
  ordem, modo`.
- [ ] **Step 4: Run** → PASS. **Commit** `feat: keep Banco filters in the URL`.

### Task 2: Filter panel components

**Files:**
- Create: `src/pages/bank/TopicFilter.tsx`, `src/pages/bank/StatusFilter.tsx`,
  `src/pages/bank/SourceFilter.tsx`, `src/pages/bank/filters.test.tsx`
- Modify: `src/pages/bank/CollectionsView.tsx` (gains a pick mode), `src/labels.ts`

**Interfaces:**
- Consumes: `api.topicsTree()`, `SelectionSummary.byTopic` / `byStatus`, `api.collections`,
  `api.attempt(id)` (for the attempt chip's title).
- Produces:

```tsx
<TopicFilter tree={TopicTree} counts={TopicCount[]} selected={TopicId[]}
  onChange={(ids: TopicId[]) => void} />
<StatusFilter counts={StatusCount[]} selected={StudyStatus[]}
  onChange={(s: StudyStatus[]) => void} />
<SourceFilter collectionId={CollectionId | null} attemptId={AttemptId | null}
  onCollection={(id: CollectionId | null) => void} onClearAttempt={() => void} />
<CollectionsView onPick={(c: CollectionSummary) => void} />   // pick mode: rows are buttons
export const studyStatusLabels: Record<StudyStatus, string>;   // labels.ts
export const sessionOrderLabels: Record<SessionOrder, string>; // labels.ts
```

Behavior:
- **TopicFilter:** the area › matter › topic tree as nested `<details>`, a checkbox at each level
  with the count from `counts` (the sum of its topics at area and matter level). Checking an area
  or matter selects all its topic IDs, and unchecking clears them. A parent with only some
  children checked is `indeterminate`. Selection is always a list of topic IDs.
- **StatusFilter:** five checkboxes in enum order with their counts.
- **SourceFilter:** shows the chosen caderno as a removable `badge` with its title, or *Escolher
  caderno*, which opens a `Modal` holding `CollectionsView` in pick mode (search, contest and
  board filters as today). With `attemptId`, a removable chip "Questões de: <attempt title>".
- **Limpar filtros** (in Bank, Task 4) resets all three.

- [ ] **Step 1: Write the failing tests** in `filters.test.tsx`:
  `checking_an_area_selects_all_its_topics`, `a_partly_checked_matter_is_indeterminate`,
  `unchecking_a_matter_clears_its_topics`, `shows_each_status_with_its_count`,
  `picking_a_caderno_reports_its_id` (open the modal, click a row, expect `onCollection("c-1")`
  and the modal closed), `removing_the_attempt_chip_clears_it`.
- [ ] **Step 2: Run** `npx vitest run src/pages/bank/filters.test.tsx` → FAIL.
- [ ] **Step 3: Implement** the three components, the pick mode and the labels.
- [ ] **Step 4: Run** → PASS. **Commit** `feat: add Banco filter panels`.

### Task 3: Result panel

**Files:**
- Create: `src/pages/bank/ResultPanel.tsx`, `src/pages/bank/ResultPanel.test.tsx`

**Interfaces:**
- Consumes: `SelectionSummary`, `api.questionCountLimits`, `api.sessionStart`,
  `api.collection(id)` (open attempt of a whole caderno), `openAttempt` (`src/navigation.ts`),
  `feedbackModes`/`modeHint`/`modeLabel` (`src/labels.ts`), `actions.ts` busy state.
- Produces:

```tsx
<ResultPanel summary={SelectionSummary} filter={QuestionFilter}
  order={SessionOrder | null} mode={FeedbackMode}
  onOrder={(o: SessionOrder) => void} onMode={(m: FeedbackMode) => void} />
```

Behavior:
- The headline is "{matching} questões", plus "{inOpenAttempt} em tentativa aberta ficam de fora"
  when that is above 0. Then the by-area and by-status breakdowns as `card-compact stat` tiles
  (counts only).
- *Quantas*: a `.input` number field with limits from
  `questionCountLimits({ kind: "selection", available: matching − inOpenAttempt })`.
- *Ordem*: a select of `summary.allowedOrders`. When the current order is missing from the list,
  show the first allowed one and report it with `onOrder`.
- *Modo*: Estudo / Simulado radios, as in the old start dialog.
- *Resolver estas* sends `{ kind: "filter", filter, count, order }` and opens the attempt.
- **A caderno on its own:** when the filter has only `collectionId`, add the choice *Todas as
  questões do caderno* (selected by default). With it, the button sends
  `{ kind: "collection", collectionId }`. If `api.collection(id)` reports an unfinished attempt,
  the button reads *Continuar* and opens it.
- Errors from `sessionStart` show in an `InlineError`.

- [ ] **Step 1: Write the failing tests**: `shows_counts_without_question_text`,
  `mentions_questions_held_by_open_attempts`, `offers_only_the_allowed_orders`,
  `starts_a_filter_session_with_count_and_order` (assert the exact `session_start` request),
  `starts_the_whole_caderno_as_a_collection_attempt`,
  `continues_the_open_attempt_of_a_whole_caderno`, `disables_start_when_nothing_is_available`
  (`matching 3, inOpenAttempt 3` → button disabled with "Todas estas questões estão em uma
  tentativa aberta."), `shows_a_start_error_inline`.
- [ ] **Step 2: Run** `npx vitest run src/pages/bank/ResultPanel.test.tsx` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** → PASS. **Commit** `feat: add the Banco result panel`.

### Task 4: Compose Banco, presets, styles

**Files:**
- Modify: `src/pages/Bank.tsx` (rewrite), `src/pages/Bank.test.tsx` (rewrite),
  `src/styles/bank.css` (rewrite on design-system tokens and classes), `src/pages/headings.test.tsx`
  if it pins Banco's headings
- Delete: `src/pages/bank/TopicTreeView.tsx`, `src/pages/bank/CollectionDetailModal.tsx` (a
  caderno's attempts move to Histórico, plan 5)

**Interfaces:**
- Consumes: Tasks 1–3, `api.selectionSummary`.
- Produces: the `presets` constant in `src/pages/bank/presets.ts`, reused by Início in plan 4:

```ts
export const presets = {
  reviews: { filter: { ...emptyFilter, statuses: ["due", "wrong"] }, order: "dueFirst" },
  errors: { filter: { ...emptyFilter, statuses: ["wrong"] }, order: "recentErrorsFirst" },
} satisfies Record<string, Partial<BancoState>>;
```

Layout: `<h1>Banco</h1>`; preset buttons *Revisões de hoje* and *Caderno de erros* (`btn`,
`aria-pressed` when the current state equals the preset); below, a two-column grid (filters
`card`, result `card`). Below 60rem the filters stack above the result. *Limpar filtros* is a
`btn-link` at the end of the filters.

- [ ] **Step 1: Rewrite `Bank.test.tsx`** with a fake backend (`mockIPC`) for `topics_tree`,
  `selection_summary` (records each filter), `collections`, `collection`,
  `question_count_limits`, `session_start`:
  `restores_filters_from_the_url`, `checking_a_status_updates_the_url_and_the_summary`,
  `a_preset_sets_filters_and_order`, `clearing_filters_resets_the_url`,
  `shows_the_latest_summary_when_replies_arrive_out_of_order` (the first `selection_summary`
  resolves after the second; the screen shows the second), `shows_a_retry_when_the_summary_fails`
  (`ResourceView` retry), `shows_the_import_hint_on_an_empty_bank` (the current empty-state copy).
- [ ] **Step 2: Run** `npx vitest run src/pages/Bank.test.tsx` → FAIL.
- [ ] **Step 3: Implement** `Bank.tsx` with `useSearchParams` ↔ `parseBanco`/`bancoSearch`, one
  `createResource` keyed on the serialized filter (Solid drops stale results for a newer
  source), the presets, and the new `bank.css`. Delete the two old views and their CSS.
- [ ] **Step 4: Run** `npm run lint && npm run typecheck && npm test` → green.
- [ ] **Step 5: Check by hand** (`npm run tauri dev`): check light and dark, a narrow window,
  choosing a caderno, a preset, starting, then Back from Resolver.
- [ ] **Step 6: Commit** `feat: rebuild Banco around filters`; then open an issue "Banco screen"
  linked to the design doc, branch `issue-<n>-banco-screen`, PR `Closes #<n>`.
