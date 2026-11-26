# Migration

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

## Intentional changes

- **Version 2, Resolver and Gerar/Revisar redesign:** stems and options grow to 18px (`reading-lg`) with more space between options; the question card becomes a printed Sheet; the explanation gets its own panel; the navigator moves to a side rail; Gerar's two-column grid becomes stacked numbered sections; Revisar's chips become filterable rows. `title` grows to 24px, `heading` to 18px, `display` to 32px; `measure` tightens from 68ch to 65ch.

- **Toast:** the coloured left border is replaced by a round status mark (✓ or !) so the status has a shape as well as a colour.
- **Selected states use `is-*`** (`is-selected`, `is-correct`) rather than mixing `.selected`, `.answered`, `.outcome-*` and `.option-key`.
- **Hard-coded paddings** (0.35rem × 0.8rem, 0.45rem × 0.9rem, 0.9rem) are folded into `space-2` × `space-4` and `space-3` × `space-4`.
- **The collapsed-sidebar rules** are written twice in `shell.css` (`.sidebar-collapsed` and the 60rem media query); write them once on a shared selector.
