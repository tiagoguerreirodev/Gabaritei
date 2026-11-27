# Gabaritei design system

The visual language of the Gabaritei UI, version 3. Version 2 described a target; the app has since built most of it, plus screens the system never covered (Início, Desempenho, Configurações, the loading and error states, the score recap). Version 3 describes the app as it ships at `main@be1af88` and brings those parts in, with the same identity.

`src/styles/` implements it: `tokens.css` is generated from `tokens.json` with `npm run tokens`, and `src/styles/ds/` holds one file per component family. `src/styles/dsDrift.test.ts` parses `components/bundle.css` and `ds/*.css` and fails when a selector or a declaration differs, so a change goes into both files in the same commit. `migration.md` lists the renames that brought the v3 families into `ds/`.

Gabaritei looks like an answer sheet (*cartão-resposta*): cool paper, blue-black ink and the pink of optical-mark print. The interface stays quiet so the question gets the attention. One accent, flat cards, and rings that fill in when chosen.

| File | What |
| --- | --- |
| `tokens.json` | Every token (colour per theme, type styles, spacing, radius, shadow, size, duration) with a usage note. Source of truth for values; `npm run tokens` writes `src/styles/tokens.css` from it. |
| `components/bundle.css` | The component classes: first `src/styles/ds/*.css` as shipped, then the v3 families. Built only from token custom properties. |
| `components/<Name>/` | Guidelines (`README.md`) and a static specimen (`preview.html`) per family. |
| `components/*Screen/` | Início, Banco, Gerar, Revisar pacote, Resolver, Desempenho, Histórico and Configurações composed from the families. |
| `migration.md` | Old class names → v3 names, and the intentional changes. |

## Principles

1. **The question is the page.** Chrome is `surface` on `bg` with `border` hairlines; colour is spent on what the user marks, never on decoration.
2. **One accent.** `accent` means "yours": your choice, the current page, the primary action, focus, your trend line. Outcomes use `success`, `danger` and `flag`, and nothing else does.
3. **Never colour alone.** Every outcome carries a glyph or a word (✓ / ✗ / ⚑, *Certo* / *Errado*); meters, charts and accuracy badges print their number.
4. **Printed, not decorated.** The answer-sheet look comes from print details only: a 3px `accent` top edge (the Sheet and the TodayCard, nowhere else), fine `rule` lines, a numbered gutter, bubble-shaped markers and tracked `label` captions on the sheet. No textures, gradients or side stripes.
5. **Flat.** Cards have a border and no shadow. Shadows are for things that float: `shadow-raised` (toast, tooltip, shortcuts panel) and `shadow-overlay` (modal).
6. **No spoilers in the UI.** `success` and the key's mark appear only after the user has answered. Before that an option is plain or `is-selected`. Banco shows counts, never stems; Histórico shows titles, progress and scores only.

## Content

- UI copy is **Brazilian Portuguese**, sentence case, direct and short: *Resolver*, *Gerar questões*, *Revisões de hoje*. Buttons are verbs: *Começar*, *Confirmar resposta*, *Exportar CSV*, *Tentar de novo*.
- Speak to the user implicitly (imperatives, no *você* where it can be avoided). No exclamation marks, no emoji.
- Domain words stay the exam world's own: *caderno*, *gabarito*, *banca*, *anular*, *Certo/Errado*, *aproveitamento*, *acerto entre respondidas*.
- Numbers use tabular numerals; percentages have no space (*72%*); counts read as "12 questões". Nothing evaluated yet prints *—*, never 0%.
- An empty screen names its next step (*Nenhuma tentativa ainda.* → *Ir para o Banco*). An error says what failed and offers *Tentar de novo*.

## Colour

| Role | Token | Use |
| --- | --- | --- |
| Ground | `bg` | Page. Hover fill for quiet controls. |
| Surface | `surface` | Cards, sidebar, modal, inputs, empty bubbles. |
| Ink | `text`, `muted` | Body and secondary text, on `bg`, `surface` and any `*-soft`. |
| Lines | `border`, `border-strong` | Hairlines and skeleton lines vs control edges (3:1). A control always uses `border-strong`. |
| Print | `rule` | Fine pink print rules: sheet header and gutter, form-section titles, link and current-step underlines. Decorative only. |
| Panel | `explain-bg` | The explanation panel after an answer. |
| Accent | `accent`, `accent-soft`, `on-accent` | Fill, tint, text on fill. |
| Outcomes | `success`/`success-soft`, `danger`/`danger-soft`, `flag` | Correct, wrong, flagged; also the good / poor / fair accuracy bands. Text on `surface` or its own soft tint. |
| Other | `focus`, `scrim` | Focus ring (alias of `accent`), modal backdrop. |

- Fill with a colour, put `on-accent` on it. That holds for `accent`, `success` and `danger` in both themes (in dark the fills are light and `on-accent` is dark).
- Tint with `*-soft`, write with `text` or the matching strong colour. Never `color-mix()` in component code.
- Every text pair above meets WCAG AA (4.5:1) in both themes; `border-strong` and `focus` meet 3:1. `src/styles/contrast.test.ts` checks them.

## Type

- **Instrument Sans** (`ui`) for everything interactive; **Source Serif 4** (`reading`) for question stems, options and explanations; system mono (`mono`) for code and keys. Both faces are self-hosted variable fonts (`@fontsource-variable`, Latin subset, which covers Portuguese); the CSP allows `font-src 'self'` only.
- Interface scale: `figure` 48 (the one lead stat), `display` 32, `title` 24, `heading` 18, `body` 16, `small` 14, `caption` 12.8, plus `label` (12px, 600, uppercase, +0.08em) for the answer sheet's printed captions only, and `axis` 11px (`0.6875rem`) for chart labels.
- Reading scale, separate from the interface: `reading-lg` 18px for stems and options, `reading` 16px for explanations and previews, `reading-sm` 14px for references. All serif, line-height 1.7 (1.6 for `reading-sm`), max `measure` (65ch).
- Sizes are rem in code (shown at the 16px default) so everything follows *Tamanho do texto* (15 / 16 / 18 / 20px). The one exception is SVG chart text, which is `0.6875rem` so it follows the setting with the rest.
- Headings: the page title is the only `h1` (`title`); every card and dialog title is an `h2` (`heading`).
- Weights: 400 and 600; 700 only for `title`, question numbers, bubble letters, recap glyphs and the current step.

## Space, shape, layout

- Spacing is a 4px scale: `space-1` to `space-6`, then `space-8`, `space-10`. Documented optical exceptions only (option 0.6rem, badge 0.125rem × 0.625rem, nav gap 0.125rem).
- Three radii: `radius-sm` for small chrome (tooltips, keys, skeleton lines), `radius-md` for every rectangle, `radius-pill` for bubbles, badges, step markers and tracks.
- **Shell:** the Nav (`sidebar-width`, icon-only at `sidebar-width-collapsed` by choice or below `breakpoint-narrow`) and a content column padded `space-8` × `space-10` (`space-6` × `space-5` when narrow). Content is left-aligned. Cards stack `space-6` apart and cap at `content-width`, or `content-width-wide` for lists, tables and stat grids. In **focus mode** (an unfinished attempt open) the nav disappears.
- **Início:** TodayCard, summary strip, then quiet cards (waiting packages, area meters, open attempts). One primary button on the page.
- **Banco:** presets on top, then `.bank-layout`: FilterPanel (about 20rem) left, ResultPanel right; stacked when narrow. Every way into a session (Início, Desempenho *Treinar*, a caderno, Histórico *Refazer*) is a link to Banco with filters set, so there is one start flow and no start dialog. Filters live in the URL.
- **Resolver:** the Sheet (`sheet-width`) centred with the Rail (`rail-width`) sticky to its right, `space-8` apart; below `breakpoint-narrow` the rail becomes a scrolling row above the sheet. The Recap replaces the sheet after *Finalizar*; Shortcuts sit in the sheet footer.
- **Gerar:** one column of numbered Form sections, Steps above them, labels above fields, short fields side by side in a `form-row`, one primary action at the end. While generating, a card with the stage Steps.
- **Revisar pacote:** verdict StatGrid, then full-width ReviewRows with verdict filters above.
- **Desempenho:** StatGrid with the lead tile, weakest topics, the weekly Chart, then the DataTable.
- **Histórico:** *Em andamento* then *Finalizadas*, each a bordered list of HistoryRows, newest first, with a Pager. Empty: a State that links to Banco.
- **Configurações:** one card per concern; SettingsRows, ChoiceGroups, a text DataTable for roles.

## Navigation

- The sidebar is organized by activity: **Início, Banco, Gerar**, **Revisar pacote** while a package waits, **Desempenho, Histórico**, and **Configurações** in the footer with the version. Resolver opens in context and has no entry.
- *Erros e revisões* is gone: its lists are the Banco presets *Revisões de hoje* and *Caderno de erros*, and its old route redirects there.

## States and motion

- Focus: `3px solid var(--focus)`, `2px` offset, on everything keyboard-reachable, including scrollable table regions. Code-only focus targets (`main`, `.modal`, the recap) get none.
- Hover: quiet controls fill with `bg`; primary fills brighten slightly. Disabled: `opacity: .6`, no pointer.
- Selected and pressed: `accent-soft` background with `accent` edge or text.
- Loading: skeleton lines in `border`, pulsing slowly. Empty and error: see State.
- Motion: `dur-fast` for colour changes, `dur-base` for the bubble pop (a correct answer only), meter fill and the toast's entrance, easing `cubic-bezier(0.2, 0, 0, 1)`. Everything stops under `prefers-reduced-motion`. In forced-colors mode, selected, current and current-step states switch to a `Highlight` outline and meter fills to `Highlight`.

## Iconography

- One hand-drawn line set (`src/components/icons.tsx`): 24px grid, drawn at 20px, `stroke-width: 1.75`, round caps and joins, `currentColor`, no fill. Eight glyphs, one per destination plus the sidebar toggle. Icons are decorative (`aria-hidden`), always beside a visible or visually hidden label.
- Status glyphs are text characters, not icons: ✓ ✗ ⚑ —.
- The app icon (`src-tauri/icons/icon.png`) is the only mark; there is no wordmark. The name "Gabaritei" is set in `ui` 700 in the nav brand row.

## Components

Thirty-one families in `components/bundle.css`. Each is one base class plus `-variant` modifiers and `is-state` flags.

- **Frame:** Shell, Nav, Card, State, Modal, Toast.
- **Controls:** Button, Badge, Field, ChoiceGroup, Meter.
- **Today (Início):** TodayCard, ItemList.
- **Choosing (Banco):** FilterPanel, FilterChip, ResultPanel.
- **Answering (Resolver):** Bubble, Option, Sheet, Explanation, Rail, Recap, Shortcuts.
- **Authoring (Gerar, Revisar pacote):** FormSection, Steps, ReviewRow.
- **Data (Desempenho):** StatGrid, DataTable, Chart.
- **History and settings:** HistoryRow (with the Pager), SettingsRow.

Retired in v3: **Tabs** (no screen used it after *Erros e revisões* was removed, so its styles are gone from `ds/` too) and the dashed **EmptyState** box (the app shows empty states plain; see State).

## App-only rules

Rules the app needs that no family above implies. Each is in `bundle.css` and in the family's README.

- **Recap closing screen:** `.recap-comparison` (previous-attempt line, muted), `.recap-topics` (topic bars list), `.recap-next` (next-step actions). See Recap.
- **Modal:** `.modal-backdrop` covers the window (`position: fixed`, `z-index: 10`); `.modal` caps at `calc(100vh - 4rem)` and scrolls. Width is `min(36rem, 100%)`.
- **Toast:** `.toasts` is the fixed stack at the bottom right (`z-index: 20`); `.toast` slides in with `toast-in`.
- **Pager:** `.pager`, the previous / next row under any paged list.
- **Sheet:** `.question-image`, an image inside a stem or option, never wider than the sheet.
- **DataTable:** `.table-scroll` wraps a table wider than its card; it is focusable and shows the focus ring.
- **ChoiceGroup:** `.choice-group.is-inline label` aligns a radio with its text in the inline layout.
- **Text helpers:** `.skip-link` (first focusable element, slides in on focus) and `.sr-only`.
- **Meter:** `.meter.compact` also at the largest text size.

## Not covered yet

- The families still styled in a per-screen stylesheet rather than `ds/` are listed in `BUNDLE_ONLY` in `dsDrift.test.ts`: Steps, SettingsRow, Recap, Shortcuts and the global motion and forced-colours rules. Moving one into `ds/` means deleting its line there.
- `src/styles/*.css` outside `ds/` holds screen-only layout (the Banco collection picker, Gerar's topic path and role picker, the generation banner, `.card` stacking). Those are arrangements of existing families, not new looks.
- The v3 families keep the app's current class names except where `migration.md` lists a rename.
