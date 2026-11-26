# Screen restyles Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the remaining screens onto the design system: Resolver (Sheet, Option, Bubble,
Explanation, Rail), Gerar (FormSection), Revisar pacote (ReviewRow, filters), and Início,
Desempenho and Configurações. Then delete the legacy styles and token aliases.

**Architecture:** One task per screen, one PR per task. Each task adds the screen's component
families from `docs/design-system/components/bundle.css` as files under `src/styles/ds/`,
rewrites the screen's markup to the structure in that family's `README.md` and the screen
preview (`components/*Screen/preview.html`), deletes the screen's old CSS file, and keeps every
behavior test green. Markup changes keep roles, names and keyboard behavior. Tests select by
role and accessible name, so they shouldn't need rewriting. Where a test pins a class or a
structure, it moves to the new one.

**Tech Stack:** Solid 1.9, CSS, Vitest + `@solidjs/testing-library`.

**Spec:** `docs/superpowers/specs/2026-11-26-frontend-navigation-rethink-design.md` (step 6) and
`docs/design-system/` (README, `migration.md` › "Intentional changes", the family READMEs and
screen previews).

## Global Constraints

- Depends on the foundations plan (tokens, shared families, `ds/` folder). Gerar, Revisar
  pacote and Resolver can go in any order; *Início, Desempenho, Configurações* goes last because
  it deletes the aliases.
- **No spoilers** (AGENTS.md rule 1): `is-correct`, `is-key`, `success` and the Explanation panel
  render only from a `reveal` the backend sent. The review preview shows stem and options only.
  Every task that touches Resolver or Revisar pacote keeps or adds a component test for this,
  and runs `spoiler-auditor`.
- **Never colour alone:** outcomes carry ✓ / ✗ / ⚑ or a word; meters print their number.
- Copy: keep today's pt-BR labels and messages unless a family README names a new one
  (*Gabarito*, *Sua resposta*, *Aprovada*, *Contestada*, *Com problemas*, *Não verificada*,
  section titles *1 Assunto*, *2 Quantidade e banca*, *3 Fontes*, *4 Avançado*).
- Keyboard: Resolver shortcuts (`A`–`E`, `C`/`E`, `Enter`, `←`/`→`, `X`+letter, `F`) are
  unchanged. Bubbles and options stay buttons with the same accessible names.
- No `color-mix()` and no px font sizes in `ds/` (the `ds.test.ts` checks from the foundations
  plan cover new files automatically).
- `npm run lint`, `npm run typecheck`, `npm test` green after every task.

## Review Focus

- **Study mode before confirming:** a selected option is `is-selected` only. No `is-correct`
  anywhere, and no Explanation panel. Test: Task 1
  `a_selected_option_shows_no_outcome_before_confirming`.
- **Simulado until finish:** no outcome bubble in the Rail and no outcome class on options
  before *Finalizar*. Test: Task 1 `exam_mode_shows_no_outcomes_until_finish`.
- **An annulled question** after reveal shows `is-annulled` in the Rail and no ✓/✗ on its
  options. Test: Task 1 `an_annulled_question_shows_no_outcome_marks`.
- **A narrow window (below 60rem) in Resolver:** the Rail becomes one scrolling row above the
  sheet and stays keyboard-reachable. Test: Task 1 `the_rail_stays_a_labelled_nav` (role
  `navigation`, name *Questões*), plus the manual check.
- **The review filter with zero matches** (for example *Com problemas 0*) shows an empty
  message, not a blank list, and *Importar* still counts every importable question. Test: Task 3
  `an_empty_filter_shows_a_message`.

---

### Task 1: Resolver (Sheet, Option, Bubble, Explanation, Rail)

**Files:**
- Create: `src/styles/ds/bubble.css`, `option.css`, `sheet.css`, `explanation.css`, `rail.css`
- Modify: `src/pages/solve/QuestionCard.tsx` (→ Sheet + Options), `Navigator.tsx` (→ Rail),
  `RecheckPanel.tsx` and the reveal markup (→ Explanation), `Resolver.tsx` (`.solve-layout`),
  `ScoreSummary.tsx`, `src/styles/reading.css` (fold into the Sheet and Explanation families),
  `src/pages/solve/Resolver.test.tsx`
- Delete: `src/styles/solve.css` (keep only rules no family covers, moved into a small
  `src/styles/resolver.css` with a comment per rule)

**Interfaces:**
- Consumes: `.btn*`, `.label`, `.meter*`, `.badge*` (foundations).
- Produces: classes `.bubble`, `.bubble-lg`, `.bubble-mark` (`.is-flag`), `.option`,
  `.option-text`, `.option-note`, `.options`, `.sheet`, `.sheet-header`, `.sheet-number`,
  `.sheet-stem`, `.sheet-footer`, `.explanation`, `.explanation-verdict`, `.explanation-body`,
  `.explanation-refs`, `.explanation-actions`, `.rail`, `.rail-progress`, `.rail-group`,
  `.rail-grid`, `.rail-legend`, `.solve-layout`; state flags `is-selected`, `is-eliminated`,
  `is-filled`, `is-current`, `is-correct`, `is-wrong`, `is-key`, `is-annulled`.

Mapping (from `migration.md`): `.option-choose` + `.option` wrapper → one `.option` button;
`.option.selected` → `.is-selected`; `.option-key` → `.is-correct` with `.is-key` bubble;
`.option-wrong` → `.is-wrong`; `.navigator*` → `.rail*` in groups of 20 (label *Q 1–20*);
`.reveal*` → `.explanation*`; `.resolver-footer` → `.sheet-footer`. Stems and options use
`reading-lg` (1.125rem serif, line-height 1.7).

- [ ] **Step 1: Write the failing tests** in `Resolver.test.tsx`:
  `a_selected_option_shows_no_outcome_before_confirming`,
  `exam_mode_shows_no_outcomes_until_finish`,
  `the_missed_key_is_marked_gabarito_after_reveal` (`.option.is-correct` with
  `.option-note` "Gabarito", and the chosen wrong one with "Sua resposta"),
  `an_annulled_question_shows_no_outcome_marks`,
  `rail_bubbles_name_their_state` (aria-label *Questão 4, respondida, marcada*),
  `the_rail_groups_questions_by_twenty` (45 questions → labels *Q 1–20*, *Q 21–40*, *Q 41–45*),
  `the_rail_stays_a_labelled_nav`.
- [ ] **Step 2: Run** `npx vitest run src/pages/solve/` → the new tests FAIL.
- [ ] **Step 3: Implement** the five families and the markup. The bubble pop animation runs on a
  confirmed correct answer only, and never under `prefers-reduced-motion`.
- [ ] **Step 4: Run** `npm run lint && npm run typecheck && npm test` → green.
- [ ] **Step 5: Spoiler audit** with `spoiler-auditor`; **check by hand** in Estudo and Simulado,
  light and dark, all four text sizes, below 60rem, compared with
  `docs/design-system/components/ResolverScreen/preview.html`.
- [ ] **Step 6: Commit** `feat: redesign Resolver as an answer sheet`; issue "Resolver redesign",
  branch `issue-<n>-resolver-redesign`, PR `Closes #<n>`.

### Task 2: Gerar (FormSection)

**Files:**
- Create: `src/styles/ds/form.css`
- Modify: `src/pages/Generate.tsx`, `src/pages/generate/*.tsx`, `src/pages/Generate.test.tsx`,
  `src/styles/pages.css` (drop `.form-grid`, `.advanced`, `.generate-form .required`)

**Interfaces:**
- Consumes: `.field*`, `.input` (foundations).
- Produces: `.form`, `fieldset.form-section`, `legend.form-section-title.label`,
  `.form-section-number`, `.form-row`, `.form-actions`.

Sections, in order: *1 Assunto* (TopicPicker), *2 Quantidade e banca* (count, board, format in a
`form-row`; instructions), *3 Fontes* (RemNote, web, attachments), *4 Avançado* (roles, models,
effort, Fast Mode) in a `details`, collapsed by default. Optional fields get "(opcional)" in
their label instead of marking required ones. One primary action *Gerar questões* in
`.form-actions`.

- [ ] **Step 1: Write the failing tests**: `the_form_has_four_numbered_sections` (legends in
  order), `advanced_starts_collapsed`, `optional_fields_say_opcional`,
  `a_validation_error_opens_advanced_when_it_is_there` (an invalid model under *Avançado*
  expands the section so the error is visible).
- [ ] **Step 2: Run** `npx vitest run src/pages/Generate.test.tsx` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `npm run lint && npm run typecheck && npm test` → green; check by hand
  against `GenerateScreen/preview.html`.
- [ ] **Step 5: Commit** `feat: redesign Gerar as numbered sections`; issue, branch
  `issue-<n>-gerar-redesign`, PR.

### Task 3: Revisar pacote (ReviewRow, filters)

**Files:**
- Create: `src/styles/ds/review.css` (the `filters`, `filter`, `filter-count`, `review-list`,
  `review-row*` sections and the `badge-success`/`badge-dashed` modifiers from `bundle.css`)
- Modify: `src/pages/ReviewPackage.tsx`, `src/pages/review/QuestionLines.tsx` (→ review rows),
  `VerdictCounts.tsx` (→ `card card-compact stat` tiles), `ReviewActions.tsx`,
  `src/pages/ReviewPackage.test.tsx`, `src/pages/bank/CollectionsView.tsx` (its old
  `.filters` toolbar class becomes `.collection-filters` so it doesn't collide)

**Interfaces:**
- Consumes: `.badge*`, `.card*`, `.btn*` (foundations).
- Produces: `.filters`, `.filter[aria-pressed]`, `.filter-count`, `.review-list`,
  `details.review-row`, `.review-row-head`, `.badge-success`, `.badge-dashed`.

Verdict labels: *Aprovada* (`badge badge-success`), *Contestada* (`badge badge-danger
badge-dashed`), *Com problemas* (`badge badge-danger`), *Não verificada* (`badge`). Filters:
*Todas*, *Contestadas*, *Com problemas*, *Aprovadas*, each with its count, and a single active
one (`aria-pressed="true"`). Only flagged rows expand, and they show the stem and options only.

- [ ] **Step 1: Write the failing tests**: `rows_show_their_verdict_in_words`,
  `filters_narrow_the_rows_and_show_counts`, `an_empty_filter_shows_a_message`
  ("Nenhuma questão neste filtro."), `a_flagged_row_expands_to_stem_and_options_only` (no key,
  explanation or reference text in the DOM), `unflagged_rows_do_not_expand`.
- [ ] **Step 2: Run** `npx vitest run src/pages/ReviewPackage.test.tsx` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `npm run lint && npm run typecheck && npm test` → green; run
  `spoiler-auditor`; check by hand against `ReviewScreen/preview.html`.
- [ ] **Step 5: Commit** `feat: redesign Revisar pacote as filterable rows`; issue, branch
  `issue-<n>-revisar-redesign`, PR.

### Task 4: Início, Desempenho, Configurações; delete the legacy layer

**Files:**
- Modify: `src/pages/Home.tsx`, `src/pages/home/TodayCard.tsx`, `src/pages/Performance.tsx`,
  `src/pages/performance/*.tsx`, `src/pages/Settings.tsx`, `src/pages/settings/*.tsx`,
  `src/styles/performance.css`, `src/styles/settings.css`, `src/styles/pages.css`,
  `src/styles.css`, `src/styles/contrast.test.ts` (if any pair still names an alias)
- Delete: `src/styles/legacy-aliases.css`, and any of `pages.css`, `performance.css`,
  `settings.css` left empty

**Interfaces:**
- Consumes: every `ds/` family.

Rules: replace each old token name (`--radius`, `--shadow-toast`, `--shadow-modal`,
`--flag-bar`, `--text-*`, `--leading-*`) with its new name or a type-style size per
`migration.md` › Tokens; replace `color-mix()` tints with `*-soft`; fold off-scale paddings into
`space-*`. Screen-only layout rules stay in a per-screen file with one comment per rule.

- [ ] **Step 1: Write the failing test** `no_legacy_token_names_remain` in
  `src/styles/ds.test.ts`: no file under `src/styles/` (and no `style=` attribute in `src/`)
  references `--radius)`, `--radius ` (bare), `--shadow-toast`, `--shadow-modal`, `--flag-bar`,
  `--text-xs|sm|base|md|lg|xl` or `--leading-`, and no file uses `color-mix(`.
- [ ] **Step 2: Run** `npx vitest run src/styles/ds.test.ts` → FAIL, listing the files.
- [ ] **Step 3: Migrate** the three screens and delete the aliases file and empty stylesheets.
- [ ] **Step 4: Run** `npm run lint && npm run typecheck && npm test` → green; check every
  screen by hand in both themes and all four text sizes.
- [ ] **Step 5: Update** `docs/design-system/README.md` (the system is now what `src/styles/`
  implements; drop the "target" sentence) and `AGENTS.md` (Stack and layout: the styles folder).
  **Commit** `refactor: finish the design-system migration`; issue, branch
  `issue-<n>-ds-cleanup`, PR.
