# FilterPanel
The left column of Banco: three filter groups that combine with AND between groups and OR inside a group (*Assuntos*, *Status*, *Caderno*), each choice showing how many questions it holds.

- Markup: `.card.filter-panel` > `fieldset.filter-group` per group, its `legend` in `label` style. Every choice is a `label.check-item` with a native checkbox, `.check-label` and `.check-count`.
- **Assuntos** is a `ul.check-tree` of nested `details` (area › matter › topic) with a check at every level. Checking an area or matter selects everything under it; a parent with only some children checked sets the checkbox's `indeterminate` property (the native dash shows it, not colour).
- **Status**: *Nunca resolvidas*, *Erradas*, *Em branco*, *Revisão vencida*, *Sinalizadas*, in that order.
- **Caderno**: a FilterChip with the chosen caderno, or a default button *Escolher caderno* that opens the caderno picker in a Modal. A past attempt filter (from Histórico *Refazer*) is a FilterChip too.
- *Limpar filtros* is a `btn-link` in `.filter-panel-footer`.
- Counts only: the panel never shows a stem, key or explanation. Counts come from Rust; the frontend never counts.
- Below 60rem the panel stacks above the ResultPanel.
