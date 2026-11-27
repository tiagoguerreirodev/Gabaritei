# Migration

## Version 3: the per-screen stylesheets → v3 families

Version 3 adopts what `main@be1af88` ships. The classes below come from `src/styles/{shell,pages,performance,settings,resolver,bank}.css`; most keep their names, and these are the renames and changes.

| Old (app today) | v3 | Why |
| --- | --- | --- |
| `.today-card` with `border-left: 4px solid accent` | `.today-card` with `border-top: 3px solid accent` | The print edge is the Sheet's top edge; a side stripe reads as a generic alert card. |
| `.step-list`, `.stage-list` | `.steps` | One family for both. The current step drops its underline: the filled bubble and bold label carry it, and the underline ran into the marker. |
| `.accuracy-chip.accuracy-good / -fair / -poor` | `.badge.badge-good / -fair / -poor` | Accuracy bands are Badge variants. |
| `.strength-table`, `.roles-table` | `.data-table`, `.data-table.is-text` | One table family. |
| `.mastery-bar` in a table cell, `.table-mastery` | `.meter-track` inside `.table-meter` | The Meter's own parts. |
| `.area-list`, `.waiting-list`, `.weakest-list` | `.item-list` | One quiet row list. |
| `.attempt-list`, `.attempt-title` | `.item-list.is-ruled`, `.item-title` | Same list, ruled when rows carry two lines and an action. |
| `.mode-choice` | `.choice-group` | Radios with a legend. |
| `.order-choice` | `.choice-group.is-inline` | Same, on one line. |
| `.backup-list` | `.settings-list` | Lists of stored items in Configurações. |
| `.card.score` | `.card.recap` | Named after the family. |
| `.chip` (uppercase caderno meta) | `.badge`, sentence case | No uppercase outside the sheet's printed labels. |
| `.tabs`, `.tab` (`src/styles/ds/tabs.css`) | removed | No screen uses tabs since *Erros e revisões* went away. |
| `.empty-state` dashed box (v2 spec, never built) | plain `.state.empty-state` > `.empty-title` | Fits the app; no extra frame. |
| `.check-row` (Atalhos checkbox) | a `label` inside `.choice-group` | Same control family as the radios. |
| `.hint` (accent hint text in Resolver) | `.field-hint` | One hint style. |

Unchanged and now documented: `.shell`, `.shell.focus-mode`, `.content`, `.page`, `.skip-link`, `.sr-only`, `.state`, `.skeleton`, `.skeleton-line`, `.state-action`, `.error`, `.error-state`, `.summary-strip`, `.stat-cards`, `.stat-card-main`, `.stat-hint`, `.meter.compact`, `.facts`, `.table-scroll`, `.trend-chart`, `.sparkline`, `.chart-note`, `.settings-row`, `.settings-status`, `.recap-*`, `.shortcuts`, `.shortcuts-panel`, `kbd`.

## Version 2: the old stylesheets → v2


How the current stylesheets (`src/styles/*.css`, about 2,240 lines) map onto this system. Apply these renames when you refactor the app.

## Tokens

| Old | New | Why |
| --- | --- | --- |
| `--radius` | `--radius-md` | Every radius named by size. |
| `--shadow-toast` | `--shadow-raised` | Named by elevation, also used by tooltips. |
| `--shadow-modal` | `--shadow-overlay` | Named by elevation. |
| `--flag-bar` | `--flag` | One flag colour; `flag` already meets 3:1 as a mark. |
| `color-mix(in srgb, var(--success) 14–25%, var(--surface))` | `--success-soft` | One tint per outcome instead of three mix ratios. |
| `color-mix(in srgb, var(--danger) 12–25%, var(--surface))` | `--danger-soft` | Same. |
| `--text-xs … --text-xl` | type styles `caption`, `small`, `body`, `heading`, `title`, `display` | Styles carry size, weight and leading together. |
| `--leading-ui`, `--leading-reading` | inside `body` and `reading` | |
| dark values written twice (media query + `[data-theme="dark"]`) | generated once from `tokens.json` | One source per value. |

## Classes

| Old | New |
| --- | --- |
| `.primary`, `.button-link.primary` | `.btn.btn-primary` |
| `.primary.danger` | `.btn.btn-danger` |
| `.button-link`, `.export-button`, `.pager button`, `.resolver-actions button`, `.resolver-footer button`, `.modal-footer button`, `.reveal-actions button` | `.btn` |
| `.icon-button`, `.sidebar-toggle`, `.flag-button`, `.shortcuts-toggle`, `.option-eliminate` | `.btn.btn-ghost` |
| `.link-button` | `.btn.btn-link` |
| `.card`, `.question-card`, `.generation-banner`, `.notice` | `.card` (+ `.card-row` for banner and notice) |
| `.card.wide` | `.card.card-wide` |
| `.stat-card` | `.card.card-compact` with `.stat` |
| `.badge` | `.badge` |
| `.tag` | `.badge.badge-accent` |
| `.disputed-badge` | `.badge.badge-danger` |
| `.menu-count`, `.count` | `.badge.badge-count` |
| `.form-grid input`, bare `input`/`select`/`textarea` | `.input` inside `.field` |
| `.hint`, `.form-grid .hint` | `.field-hint` |
| `.field-error`, `.error` | `.field-error` |
| `.option-bubble`, `.navigator button` | `.bubble` / `.bubble.bubble-lg` |
| `.navigator .answered`, `.option.selected .option-bubble` | `.bubble.is-filled` |
| `.outcome-correct`, `.option-key` | `.is-correct` / `.is-key` |
| `.outcome-wrong`, `.option-wrong` | `.is-wrong` |
| `.navigator .current` | `.bubble.is-current` |
| `.nav-mark`, `.nav-flag` | `.bubble-mark`, `.bubble-mark.is-flag` |
| `.option.selected`, `.option.eliminated` | `.option.is-selected`, `.option.is-eliminated` |
| `.option-choose` + `.option` wrapper | one `.option` button |
| `.mastery*`, `.progress-bar`/`.progress-fill` | `.meter`, `.meter-track`, `.meter-fill`, `.meter-value` |
| `.sidebar`, `.menu-link`, `.brand` | `.nav`, `.nav-item`, `.nav-brand` |
| `.toast` with `border-left` accent | `.toast` with a round `.toast-mark` glyph |
| `.question-card`, `.question-header`, `.question-stem` (Resolver) | `.sheet`, `.sheet-header`, `.sheet-stem` + `.sheet-number` |
| `.reveal`, `.reveal h3`, `.explanation`, `.refs`, `.reveal-actions` | `.explanation`, `.label` section titles, `.explanation-body`, `.explanation-refs`, `.explanation-actions` |
| `.navigator`, `.navigator-group`, `.navigator-label`, `.navigator-bubbles`, `.progress-bar` | `.rail`, `.rail-group`, `.label`, `.rail-grid`, `.rail-progress` |
| `.resolver-footer` | `.sheet-footer` |
| `.form-grid` (11rem label column), `.advanced` | `.form` > `fieldset.form-section` with stacked `.field`s and `.form-row`; *Avançado* as the last section |
| `.generate-form .required` | "(opcional)" on optional labels instead |
| `.question-lines` chips, `.line-disputed`, `.line-unchecked` | `.review-list` > `details.review-row` with verdict badges |
| `.verdict-counts` | `.card.card-compact.stat` tiles |
| `.curate-toolbar` | `.filters` > `.filter[aria-pressed]` |
| `.tabs` on Banco (*Assuntos* / *Cadernos*), `.tree*`, `.tree-leaf .primary`, `.tree-stats`, `.mastery.compact` | Banco FilterPanel: `.filter-panel`, `.filter-group`, `.check-tree`, `.check-item`, `.check-count` |
| `.filters` (Cadernos list search, contest and board) | the caderno picker inside a Modal, opened from the FilterPanel; the chosen caderno shows as a `.filter-chip` |
| `.chip` (uppercase caderno meta) | `.badge` |
| `.score-chip[data-band]` | the printed percentage in `.history-score` (no colour band) |
| `.collection-card*`, `.facts` (collection detail) | caderno picker rows; a caderno's attempts move to Histórico `.history-row` |
| `StartSessionModal`: `.mode-choice`, `.count-field` | Banco ResultPanel: `.result-settings` with a number `.field`, an order select and `.radio-row` |
| `.learning-actions`, `.learning-list`, `.learning-topic`, `.learning-blanks` and the Learning tabs | removed with Erros e revisões; the lists are the Banco presets *Revisões de hoje* and *Caderno de erros* |
| `.pager` (Banco) | `.pager` (Histórico and any paged list) |
| `.state` empty texts, `.empty-title` | `.empty-state`, `.empty-state-title` |

## Intentional changes

- **Navigation rethink (#226):** Banco becomes filters plus a result panel and replaces the start-session dialog; Erros e revisões is removed (its lists are Banco presets); Histórico is new; the sidebar order is Início, Banco, Gerar, Desempenho, Histórico, Revisar pacote, Configurações. New families: FilterPanel, FilterChip, ResultPanel, HistoryRow, EmptyState, Pager; default buttons gain a pressed state for the presets. Banco's new class names avoid today's `.chip`, `.check-row` and `.empty-title`, so the screens can switch one at a time.

- **Version 2, Resolver and Gerar/Revisar redesign:** stems and options grow to 18px (`reading-lg`) with more space between options; the question card becomes a printed Sheet; the explanation gets its own panel; the navigator moves to a side rail; Gerar's two-column grid becomes stacked numbered sections; Revisar's chips become filterable rows. `title` grows to 24px, `heading` to 18px, `display` to 32px; `measure` tightens from 68ch to 65ch.

- **Toast:** the coloured left border is replaced by a round status mark (✓ or !) so the status has a shape as well as a colour.
- **Selected states use `is-*`** (`is-selected`, `is-correct`) rather than mixing `.selected`, `.answered`, `.outcome-*` and `.option-key`.
- **Hard-coded paddings** (0.35rem × 0.8rem, 0.45rem × 0.9rem, 0.9rem) are folded into `space-2` × `space-4` and `space-3` × `space-4`.
- **The collapsed-sidebar rules** are written twice in `shell.css` (`.sidebar-collapsed` and the 60rem media query); write them once on a shared selector.
