# Button
One button base, `.btn`, with four looks: default (secondary), `btn-primary`, `btn-danger`, `btn-ghost`, and `btn-link`.

- **Primary**: the one main action of a view (*Começar*, *Confirmar resposta*). At most one per card or modal.
- **Default**: every other action (*Exportar CSV*, *Anterior*, *Cancelar*). Its edge is `border-strong`.
- **Danger**: destructive confirmations only (*Abandonar tentativa*), inside a confirm modal.
- **Toggle**: a default button with `aria-pressed` reads as selected when pressed (`accent` on `accent-soft`), as the Banco presets do. It may carry a `badge-count`.
- **Ghost**: icon-only or toggle controls (sidebar toggle, flag, eliminate option, close). Give it an `aria-label`; a toggle sets `aria-pressed`.
- **Link**: a quiet way onward (*Ver caderno*, *Ver todas*). `accent` text at 600 with a 2px `rule` underline that turns `accent` on hover, and a trailing → that nudges right. It has the same height as the other buttons, so it lines up with them in a row. Inside running text or a tree row add `is-inline`: no arrow, regular weight, no padding.

Use `<button>` for actions and `<a class="btn">` for navigation. Labels are verbs in sentence case. Disabled is `opacity .6`; prefer explaining why over disabling silently.
