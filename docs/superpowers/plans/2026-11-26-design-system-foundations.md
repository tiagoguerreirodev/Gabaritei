# Design-system foundations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put the design system's tokens and shared component families into the app and move
every screen's shared chrome (buttons, cards, badges, fields, tabs, meters, modals, toasts, the
sidebar) onto them, with no flow or route change.

**Architecture:** `docs/design-system/tokens.json` becomes the single source of token values; a
small TypeScript script generates `src/styles/tokens.css` from it, and a test fails when the two
drift. The shared class families of `docs/design-system/components/bundle.css` are copied into
one file per family under `src/styles/ds/`. Old shared rules are deleted as each family lands.
Screen-only families (Sheet, Option, Bubble, Explanation, Rail, FormSection, ReviewRow, filters)
are **not** added here; they arrive with their screens in later plans, because their class
names collide with today's screen styles (`.option`, `.options`, `.explanation`, `.filters`).

**Tech Stack:** Solid 1.9, Vite 8, Vitest 5 + jsdom, plain CSS, Node 26 (runs `.ts` scripts
natively).

**Spec:** `docs/superpowers/specs/2026-11-26-frontend-navigation-rethink-design.md` (step 1 of
"Order of work"), with `docs/design-system/README.md` and `docs/design-system/migration.md` as
the visual target.

## Global Constraints

- No user-visible flow, route or copy change. Visual changes are only those in
  `docs/design-system/migration.md` ("Intentional changes") and the token values in
  `tokens.json`.
- CSP: no remote content. Never import the Google Fonts URL at the top of `bundle.css`; fonts stay
  self-hosted through `src/styles/fonts.css`.
- Text size: never set `font-size` in px on `:root` (bundle.css does, for previews). The
  `data-text-size` rules (15 / 16 / 18 / 20px) stay the only root sizes.
- No `color-mix()` in app CSS for outcome tints; use `--success-soft` / `--danger-soft`.
- Every text pair in both themes meets 4.5:1; `border-strong`, `focus` and `flag` as a mark meet
  3:1 (`docs/design-system/README.md`, Colour).
- Light is the first theme; dark values are emitted twice (system preference and explicit
  `data-theme="dark"`), from one source.
- UI text pt-BR; code and comments English. Prettier `printWidth` 100.
- `npm run lint`, `npm run typecheck`, `npm test` green after every task.

## Review Focus

- **Text size *Muito grande* (20px)** must still scale every new family: all new sizes are rem.
  Test: Task 2 asserts no `px` font-size in `src/styles/ds/*.css`.
- **Explicit theme vs system theme**: a user on *Escuro* with a light OS must get every dark
  value, including the new `rule`, `explain-bg`, `*-soft`. Test: Task 1 checks the two dark
  blocks are identical and cover every themed token.
- **Collapsed sidebar below 60rem**: icons only, labels as tooltips, toggle hidden, exactly as
  today. Test: Task 6 Layout test with a narrow `matchMedia`.
- **Toast for screen readers**: the new ✓ / ! mark is decorative; the `role` and text stay.
  Test: Task 5 asserts the mark is `aria-hidden` and the alert text is unchanged.
- **Disabled primary buttons** (busy states in Gerar, Configurações) keep `disabled` and the
  0.6 opacity look. No test needed beyond existing busy-state tests, which must stay green.

---

### Task 1: Tokens from `tokens.json`

**Files:**
- Create: `scripts/tokens.ts`, `scripts/generate-tokens.ts`, `src/styles/theme.css`,
  `src/styles/legacy-aliases.css`, `src/styles/tokens.test.ts`
- Modify: `src/styles/tokens.css` (now generated), `src/styles.css`, `package.json`,
  `tsconfig.node.json` (`include` adds `scripts/*.ts`), `src/styles/contrast.test.ts`

**Interfaces:**
- Produces: `renderTokensCss(tokens: DesignTokens): string` and the `DesignTokens` type in
  `scripts/tokens.ts`; `npm run tokens` rewrites `src/styles/tokens.css`. Custom property names
  are the token names (`--bg`, `--space-4`, `--radius-md`, `--shadow-raised`, `--sheet-width`,
  `--dur-fast`…), plus `--font-ui`, `--font-reading`, `--font-mono` from `type.families` and
  `--ease: cubic-bezier(0.2, 0, 0, 1)`.

- [ ] **Step 1: Write the failing tests in `src/styles/tokens.test.ts`**

```ts
import tokens from "../../docs/design-system/tokens.json";
import { renderTokensCss } from "../../scripts/tokens";

it("committed tokens.css matches tokens.json (run npm run tokens)", () => {
  expect(readFileSync("src/styles/tokens.css", "utf8")).toBe(renderTokensCss(tokens));
});
it("renders an alias as a var() reference", () => {
  expect(renderTokensCss(tokens)).toContain("--focus: var(--accent);");
});
it("writes the same dark values for the system preference and data-theme=dark", () => {
  // parse both dark blocks of the rendered CSS; expect equal and to contain every token
  // whose value has a "dark" key (bg, surface, …, rule, explain-bg, success-soft, danger-soft,
  // scrim, shadow-raised, shadow-overlay)
});
```

- [ ] **Step 2: Run `npx vitest run src/styles/tokens.test.ts`**

Expected: FAIL, `scripts/tokens` not found.

- [ ] **Step 3: Implement `renderTokensCss` in `scripts/tokens.ts`**

Output, in order: a header comment `/* Generated from docs/design-system/tokens.json by npm run
tokens. Do not edit. */`; `:root { … }` with every token's light (or single) value, the font
families and `--ease`; `@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) {
… } }` and `:root[data-theme="dark"] { … }` with the dark values only. `"{name}"` becomes
`var(--name)`. Values are written as in the JSON (no unit conversion). Type styles are not
emitted (components carry their sizes).
`scripts/generate-tokens.ts` reads the JSON, writes the file, and runs Prettier on it so
`npm run lint` passes. Add `"tokens": "node scripts/generate-tokens.ts"` to `package.json`.

- [ ] **Step 4: Split the hand-written parts out of tokens.css**

Move `color-scheme` (root and per `data-theme`), the root `font-family`, `line-height: 1.5`,
`color`, `background` and the three `data-text-size` rules into `src/styles/theme.css`, keeping
the root `font-size: 16px` default there. Put the old names that screens still use in
`src/styles/legacy-aliases.css` (header: "Delete when the last screen has migrated"):
`--radius: var(--radius-md)`, `--shadow-toast: var(--shadow-raised)`,
`--shadow-modal: var(--shadow-overlay)`, `--flag-bar: var(--flag)`, `--text-xs: 0.8rem`,
`--text-sm: 0.875rem`, `--text-base: 1rem`, `--text-md: 1.1rem`, `--text-lg: 1.25rem`,
`--text-xl: 1.6rem`, `--leading-ui: 1.5`, `--leading-reading: 1.7`. `src/styles.css` imports
`fonts, tokens, legacy-aliases, theme, base`, then the screen files as today.

- [ ] **Step 5: Run `npm run tokens`, then the tests**

Run: `npx vitest run src/styles/`
Expected: tokens tests PASS.

- [ ] **Step 6: Point `contrast.test.ts` at `tokens.json`**

Read themes from the JSON instead of parsing CSS (drop the CSS block parser and the "dark copies
match" test, now in `tokens.test.ts`). Pairs: keep the existing list with `flag-bar` replaced by
`flag`, and replace every `color-mix` row with a plain pair: `text`, `muted`, `success` on
`success-soft`; `text`, `muted`, `danger` on `danger-soft`; `flag` on both soft tints (3:1);
add `text` and `muted` on `explain-bg`, and `rule` is not checked (decorative).
Run: `npx vitest run src/styles/contrast.test.ts` → PASS in *claro* and *escuro*.

- [ ] **Step 7: Check and commit**

Run: `npm run lint && npm run typecheck && npm test` → all green. The app looks the same except
`measure` 65ch.

```bash
git add scripts src/styles src/styles.css package.json tsconfig.node.json
git commit -m "feat: generate the CSS tokens from the design system"
```

### Task 2: Base, Button and the shared text classes

**Files:**
- Create: `src/styles/ds/base.css`, `src/styles/ds/button.css`, `src/styles/ds.test.ts`
- Modify: `src/styles.css`, `src/styles/base.css`, `src/styles/shell.css`, every `.tsx` using
  the old button classes (13 files use `primary`; `grep -rn 'primary\|button-link\|icon-button\|
  link-button\|sidebar-toggle\|flag-button\|shortcuts-toggle\|option-eliminate\|export-button'
  src --include=*.tsx`)

**Interfaces:**
- Produces: classes `.btn`, `.btn-primary`, `.btn-danger`, `.btn-ghost`, `.btn-link`
  (`.is-inline`), `.muted`, `.reading`, `.label`.

- [ ] **Step 1: Write the failing test `src/styles/ds.test.ts`**

```ts
it("ds files use rem font sizes only, so the text-size setting scales them", () => {
  for (const file of readdirSync("src/styles/ds")) {
    expect(readFileSync(`src/styles/ds/${file}`, "utf8")).not.toMatch(/font-size:\s*[\d.]+px/);
  }
});
it("ds files load nothing remote", () => {
  // no "@import url(" and no "http" in any ds file
});
```

Run: `npx vitest run src/styles/ds.test.ts` → FAIL (folder missing).

- [ ] **Step 2: Create `ds/base.css` and `ds/button.css`**

Copy from `bundle.css`: the `:root`/`*`/`body`/`a`/`:focus-visible` block **minus** the Google
Fonts `@import` and the root `font-size`/`--ease` (already in `theme.css`/tokens), then
`.muted`, `.reading`, `.label`, and the whole Button section. Import both in `src/styles.css`
after `theme.css` and before the screen files. Delete the old `.primary`, `.button-link`,
`.icon-button`, `.link-button`, `.muted` and focus rules from `base.css`/`shell.css`, and the
screen-scoped button rules listed in `migration.md` (`.pager button`, `.resolver-actions button`,
`.resolver-footer button`, `.modal-footer button`, `.reveal-actions button`, `.export-button`).

- [ ] **Step 3: Rename the classes in the `.tsx` files per `migration.md` › Classes**

`primary` → `btn btn-primary`; `primary danger` → `btn btn-danger`; plain action buttons →
`btn`; icon-only buttons → `btn btn-ghost`; `link-button` → `btn btn-link`. Tests that select by
class get the new names; tests that select by role need no change.

- [ ] **Step 4: Run the checks**

Run: `npm run lint && npm run typecheck && npm test` → green.

- [ ] **Step 5: Commit**

```bash
git commit -am "refactor: move buttons onto the design-system classes"
```

(Use `git add` for the new `src/styles/ds/` files first.)

### Task 3: Card, Stat and Badge

**Files:**
- Create: `src/styles/ds/card.css`, `src/styles/ds/badge.css`
- Modify: `src/styles.css`, `src/styles/shell.css`, `src/styles/performance.css`,
  `src/styles/pages.css`, the `.tsx` files using `card`, `stat-card`, `badge`, `tag`,
  `disputed-badge`, `menu-count`, `count`, `generation-banner`, `notice`

**Interfaces:**
- Produces: `.card`, `.card-wide`, `.card-compact`, `.card-row`, `.stat`, `.stat-name`,
  `.stat-value`, `.badge`, `.badge-accent`, `.badge-danger`, `.badge-count`.

- [ ] **Step 1: Copy the Card and Badge sections of `bundle.css` into the two files and import
  them**
- [ ] **Step 2: Delete the old rules** (`.card`, `.card h2`, `.card.wide`, `.stat-card`,
  `.badge`, `.tag`, `.disputed-badge`, `.count`, `.menu-count` base look) **and rename in TSX**
  per `migration.md`: `card wide` → `card card-wide`; `stat-card` → `card card-compact stat`;
  `tag` → `badge badge-accent`; `disputed-badge` → `badge badge-danger`; `count`/`menu-count` →
  `badge badge-count`; `generation-banner` and `notice` → `card card-row`. Page-specific rules
  that only position a card (`.card.today-card h2`) stay in their screen file.
- [ ] **Step 3: Run `npm run lint && npm run typecheck && npm test`** → green.
- [ ] **Step 4: Commit** `refactor: move cards and badges onto the design-system classes`

### Task 4: Field and Tabs

**Files:**
- Create: `src/styles/ds/field.css`, `src/styles/ds/tabs.css`
- Modify: `src/styles.css`, `src/styles/base.css`, `src/styles/bank.css`,
  `src/styles/pages.css`, `src/styles/settings.css`, `.tsx` files with `hint`, `field-error`,
  `error` (form error text), bare `input`/`select`/`textarea`, `src/pages/Bank.tsx`

**Interfaces:**
- Produces: `.field`, `.field-label`, `.field-hint`, `.field-error`, `.input`
  (`[aria-invalid="true"]`), `.tabs`, `.tab` (`[aria-selected="true"]`).

- [ ] **Step 1: Copy the Field and Tabs sections; import them**
- [ ] **Step 2: Wrap each form control in `.field`** with `.field-label`, give the control
  `class="input"`, rename `hint` → `field-hint` and form `error` → `field-error`. Keep Gerar's
  `.form-grid` layout as is: its replacement (`.form` / FormSection) belongs to the Gerar screen
  plan. Delete the old bare-element input rules from `base.css` and the `.tabs`/`.tab` rules from
  `bank.css`.
- [ ] **Step 3: Run `npm run lint && npm run typecheck && npm test`** → green. Field tests that
  check `aria-invalid` and error text (Generate, Settings) must pass unchanged.
- [ ] **Step 4: Commit** `refactor: move form fields and tabs onto the design-system classes`

### Task 5: Modal, Toast and Meter

**Files:**
- Create: `src/styles/ds/modal.css`, `src/styles/ds/toast.css`, `src/styles/ds/meter.css`
- Modify: `src/styles.css`, `src/styles/shell.css`, `src/styles/solve.css`,
  `src/styles/performance.css`, `src/components/toasts.tsx`, `src/components/MasteryBar.tsx`,
  `src/components/GenerationProgress.tsx`, `src/pages/solve/Navigator.tsx` (progress bar only)
- Test: `src/components/shell.test.tsx` (toasts), existing MasteryBar/GenerationProgress tests

**Interfaces:**
- Produces: `.modal-backdrop`, `.modal`, `.modal-header`, `.modal-body`, `.modal-footer`,
  `.toast`, `.toast-mark`, `.toast-text`, `.toast-error`, `.meter`, `.meter-track`,
  `.meter-fill`, `.meter-value`.

- [ ] **Step 1: Write the failing toast test** in `src/components/shell.test.tsx`

```ts
it("marks a toast's status with a decorative glyph", async () => {
  // showToast("Backup feito."); showError(new Error("Falhou."))
  // success toast: .toast-mark text "✓", aria-hidden="true"; role "status", text "Backup feito."
  // error toast: .toast-mark text "!", aria-hidden="true"; role "alert", text "Falhou."
});
```

Run it → FAIL (no `.toast-mark`).

- [ ] **Step 2: Copy the Modal, Toast and Meter sections; import them.** In `toasts.tsx`, render
  `<span class="toast-mark" aria-hidden="true">{tone === "error" ? "!" : "✓"}</span>` before
  `<span class="toast-text">`. Delete the old `.toast` left-border rules and the old modal rules
  in `shell.css` and `solve.css` (keep `.modal:focus { outline: none }`, the code-only focus
  target rule, in `ds/modal.css`).
- [ ] **Step 3: Move `.mastery*` and `.progress-bar`/`.progress-fill` to `.meter*`** per
  `migration.md`; the meter prints its number in `.meter-value` (README principle 3). Existing
  tests that read the percentage text must stay green.
- [ ] **Step 4: Run `npm run lint && npm run typecheck && npm test`** → green.
- [ ] **Step 5: Commit** `refactor: move modals, toasts and meters onto the design system`

### Task 6: Nav (the sidebar shell)

**Files:**
- Create: `src/styles/ds/nav.css`
- Modify: `src/components/Layout.tsx`, `src/styles/shell.css`, `src/styles/shell.test.ts`,
  `src/test/setup.ts` (a `matchMedia` stub if jsdom lacks one)
- Test: `src/components/Layout.test.tsx`

**Interfaces:**
- Consumes: `.badge-count` (Task 3).
- Produces: `.nav`, `.nav-brand`, `.nav-item` (`[aria-current="page"]`), `.icon`; the shell
  root carries `data-nav="collapsed"` when collapsed, which later screen plans can rely on.

- [ ] **Step 1: Write the failing tests in `Layout.test.tsx`**

```ts
it("collapses the nav when the window is narrower than 60rem", () => {
  // stub matchMedia("(max-width: 60rem)") → matches: true; render Layout
  // expect(container.querySelector(".shell")).toHaveAttribute("data-nav", "collapsed");
  // expect(screen.queryByRole("button", { name: "Menu lateral" })).toBeNull();
});
it("collapses the nav when the user collapses it", async () => {
  // matches: false; click "Menu lateral"; expect data-nav="collapsed"; click again → absent
});
it("marks the current page link with aria-current", () => {
  // at "/banco": link "Banco" has aria-current="page"; "Início" has none
});
```

Run → FAIL.

- [ ] **Step 2: Implement in `Layout.tsx`**

`const narrow = mediaQuery("(max-width: 60rem)")` (a small signal over `matchMedia` with a
`change` listener, cleaned up `onCleanup`); `data-nav={collapsed() || narrow() ? "collapsed" :
undefined}` on `.shell`; the toggle renders only when `!narrow()`. `<aside class="nav">`,
`nav-brand`, and `MenuLink` uses `class="nav-item"` (the router's `A` already sets
`aria-current="page"` on the active link; drop `.menu-link.active` styling). Menu counts use
`badge badge-count`.

- [ ] **Step 3: Write the collapsed rules once in `ds/nav.css`**, keyed only on
  `.shell[data-nav="collapsed"]` (width `--sidebar-width-collapsed`, labels visually hidden and
  shown as a tooltip with `surface`, `shadow-raised`, `radius-sm` on hover or focus, the count
  pinned to the icon's corner). Delete the duplicated `.sidebar-collapsed` and
  `@media (max-width: 60rem)` sidebar rules from `shell.css`; keep the `.content` padding change
  in that media query.
- [ ] **Step 4: Update `shell.test.ts`** to the new rules: `.nav-item[aria-current="page"]` uses
  `--accent-soft`; the label-tooltip rule appears exactly once (under
  `.shell[data-nav="collapsed"]`).
- [ ] **Step 5: Run `npm run lint && npm run typecheck && npm test`** → green.
- [ ] **Step 6: Check the app by hand**

Run: `npm run tauri dev`. Check light and dark, the four text sizes, a narrow window, and the
collapse toggle. Expected: the sidebar looks like `docs/design-system/components/Nav/preview.html`.

- [ ] **Step 7: Commit** `refactor: move the sidebar onto the design-system Nav`

### Task 7: Docs and PR

**Files:**
- Modify: `AGENTS.md` ("Stack and layout": `src/styles/ds/`, `scripts/tokens.ts`,
  `npm run tokens`, tokens come from `docs/design-system/tokens.json`),
  `docs/design-system/README.md` (first paragraph: foundations implemented, screen families
  pending)

- [ ] **Step 1: Update the two docs**
- [ ] **Step 2: Run the preflight skill** (all Rust and frontend checks; Rust is untouched but
  CI runs it)
- [ ] **Step 3: Commit** `docs: record where the design-system styles live`, then open the PR
  on a branch named after its issue (open an issue "Design-system foundations" first, linked to
  the design doc), `Closes #<n>`.
