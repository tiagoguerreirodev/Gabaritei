# SettingsRow
One configurable thing on Configurações: label, field, action, and a status line under the field. `div.settings-row`, a four-column grid (`settings-label-width`, the field, then up to two buttons).

- Example: *Claude Code* · executable path `input` · *Testar* · *Procurar*, then `p.settings-status` *Conectado (2.1.4)* in `success` (`.ok`) or `p.error.settings-status` with the reason.
- A status is a word, never a colour dot alone. It appears only after a test ran.
- Below `breakpoint-compact` the row stacks into one column.
- A list of stored items (backups) is `ul.settings-list`: hairline-separated rows, name left, actions right.
- Each Configurações section is a `card` with an `h2`; a destructive action in it is a `btn-danger` behind a ConfirmModal.
