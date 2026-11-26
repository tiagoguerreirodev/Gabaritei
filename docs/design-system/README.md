# Gabaritei design system

The visual language of the Gabaritei UI: tokens, components and usage rules. It is a target for
the frontend, not yet what `src/styles/` implements; `migration.md` maps the current stylesheets
onto it.

| File | What |
| --- | --- |
| `tokens.json` | Every token (colour per theme, type styles, spacing, radius, shadow, size, duration) with a usage note. Source of truth for values. |
| `components/bundle.css` | One class family per component, built only from token custom properties (`--bg`, `--space-4`, `--font-ui`…). |
| `components/<Name>/README.md` | Guidelines: when to use it, structure, states, do and don't. |
| `components/<Name>/preview.html` | A static HTML specimen. Line 1 is a preview marker; open it with the tokens and `bundle.css` loaded. |
| `components/*Screen/` | Resolver, Gerar and Revisar pacote composed from the components. |
| `migration.md` | Old token and class names → new ones, and the intentional changes. |

Assets are not copied here: the app icon lives in `src-tauri/icons/` and the line icons in
`src/components/icons.tsx`.

Gabaritei looks like an answer sheet (*cartão-resposta*): cool paper, blue-black ink and the pink of optical-mark print. The interface stays quiet so the question gets the attention. One accent, flat cards and rings that fill in when chosen.

## Principles

1. **The question is the page.** Chrome is `surface` on `bg` with `border` hairlines; colour is spent on what the user marks, never on decoration.
2. **One accent.** `accent` means "yours": your choice, the current page, the primary action, focus. Outcomes use `success`, `danger` and `flag`, and nothing else does.
3. **Never colour alone.** Every outcome carries a glyph or a word too (✓ / ✗ / ⚑, *Certo* / *Errado*); meters print their number.
4. **Printed, not decorated.** The answer-sheet look comes from print details only: a 3px `accent` top edge, fine `rule` lines, a numbered gutter and tracked `label` captions. No textures, gradients or extra colour.
5. **Flat.** Cards have a border and no shadow. Shadows are for things that float: `shadow-raised` (toast, tooltip) and `shadow-overlay` (modal).
6. **No spoilers in the UI.** `success` and the key's mark appear only after the user has answered. Before that, an option is plain or `is-selected`.

## Content

- UI copy is **Brazilian Portuguese**, sentence case, direct and short: *Resolver*, *Gerar questões*, *Revisões de hoje*. Buttons are verbs: *Começar*, *Confirmar resposta*, *Exportar CSV*.
- Speak to the user implicitly (imperatives, no *você* where it can be avoided). No exclamation marks, no emoji.
- Domain words stay the exam world's own: *caderno*, *gabarito*, *banca*, *anular*, *Certo/Errado*, *aproveitamento*.
- Numbers use tabular numerals; percentages have no space (*72%*); counts read as "12 questões".

## Colour

| Role | Token | Use |
| --- | --- | --- |
| Ground | `bg` | Page. Hover fill for quiet controls. |
| Surface | `surface` | Cards, sidebar, modal, inputs, empty bubbles. |
| Ink | `text`, `muted` | Body and secondary text, on `bg`, `surface` and any `*-soft`. |
| Lines | `border`, `border-strong` | Hairlines vs control edges (3:1). A control always uses `border-strong`. |
| Print | `rule` | Fine pink print rules: sheet header and gutter, form-section titles, page-end action bars. Decorative only. |
| Panel | `explain-bg` | The explanation panel after an answer. |
| Accent | `accent`, `accent-soft`, `on-accent` | Fill, tint, text on fill. |
| Outcomes | `success`/`success-soft`, `danger`/`danger-soft`, `flag` | Correct, wrong, flagged. Text on `surface` or its own soft tint. |
| Other | `focus`, `scrim` | Focus ring (alias of `accent`), modal backdrop. |

- Fill with a colour, put `on-accent` on it. That holds for `accent`, `success` and `danger` in both themes (in dark, the fills are light and `on-accent` is dark).
- Tint with `*-soft`, write with `text` or the matching strong colour. Never `color-mix()` in component code.
- Every text pair above meets WCAG AA (4.5:1) in both themes; `border-strong` and `focus` meet 3:1.

## Type

- **Instrument Sans** (`ui`) for everything interactive; **Source Serif 4** (`reading`) for question stems, options and explanations; system mono (`mono`) for code and keys. Both faces are self-hosted variable fonts in the app (Latin subset covers Portuguese).
- Interface scale: `display` 32, `title` 24, `heading` 18, `body` 16, `small` 14, `caption` 12.8, plus `label` (12px, 600, uppercase, +0.08em tracking), the printed-form caption for sheet headers, section titles and group labels.
- Reading scale, separate from the interface: `reading-lg` 18px for stems and options, `reading` 16px for explanations and previews, `reading-sm` 14px for references. All serif, line-height 1.7 (1.6 for `reading-sm`), max `measure` (65ch).
- Sizes are rem in code (shown at the 16px default) so everything follows the *Tamanho do texto* setting (15 / 16 / 18 / 20px).
- Weights: 400 and 600; 700 only for `title`, question numbers and bubble letters.

## Space, shape, layout

- Spacing is a 4px scale: `space-1` to `space-6`, then `space-8`, `space-10`. No off-scale paddings except the documented optical ones (option 0.6rem, badge 0.125rem × 0.625rem).
- Three radii: `radius-sm` for small chrome, `radius-md` for every rectangle, `radius-pill` for bubbles, badges and tracks.
- Layout: sidebar (`sidebar-width`, collapsing to `sidebar-width-collapsed` below 60rem) plus a content column padded `space-8` × `space-10`. Cards cap at `content-width`, or `content-width-wide` for tables and stat grids.
- **Resolver:** the Sheet (`sheet-width`) centred, the navigator Rail (`rail-width`) sticky to its right, `space-8` apart. Below 60rem the rail becomes a scrolling row above the sheet.
- **Forms (Gerar):** one column of numbered Form sections, labels above fields, short fields side by side in a `form-row`, one primary action at the end.
- **Lists to triage (Revisar pacote):** full-width rows with verdict filters above; never a grid of chips.

## States and motion

- Focus: `3px solid var(--focus)`, `2px` offset, on everything keyboard-reachable. Code-only focus targets (`main`, `.modal`) get none.
- Hover: quiet controls fill with `bg`; primary fills brighten slightly. Disabled: `opacity: .6`, no pointer.
- Selected: `accent-soft` background with `accent` edge or text.
- Motion: `dur-fast` for colour changes, `dur-base` for the bubble pop (a correct answer only) and meter fill, easing `cubic-bezier(0.2, 0, 0, 1)`. Everything stops under `prefers-reduced-motion`. In forced-colors mode, selected and current states switch to a `Highlight` outline.

## Iconography

- One hand-drawn line set (`src/components/icons.tsx`): 24px grid, drawn at 20px, `stroke-width: 1.75`, round caps and joins, `currentColor`, no fill. Icons are always decorative (`aria-hidden`), with a visible or visually hidden label next to them.
- Status glyphs are text characters, not icons: ✓ ✗ ⚑.
- The app icon (`src-tauri/icons/icon.png`) is the only mark; there is no wordmark. Set the name "Gabaritei" in `title`.

## Components

Sixteen class families in `components/bundle.css`: **Button, Card, Badge, Field, Tabs, Bubble, Option, Meter, Nav, Modal, Toast**, plus the answering and authoring set **Sheet, Explanation, Rail, FormSection, ReviewRow**. Each is one base class plus `-variant` modifiers and `is-state` flags. Three screen previews (*ResolverScreen*, *GenerateScreen*, *ReviewScreen*) show them composed. `migration.md` maps every old class to its new name.
