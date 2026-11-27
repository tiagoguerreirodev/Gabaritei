# DataTable
A table of figures: the area › matéria › assunto breakdown on Desempenho, the agent roles on Configurações. `table.data-table` inside `div.table-scroll` (focusable region, labelled by the card's `h2`).

- Hairline rows (`border`), no zebra, no vertical rules. Header cells in `small` muted.
- Numbers align right with tabular numerals; text columns align left. `.is-text` makes every column left-aligned (the roles table).
- Hierarchy reads by indent: `tr.level-area` (600), `tr.level-matter` (`space-8`), `tr.level-topic` (`space-8` + `space-5`). The row header is a `th scope="row"`.
- Achievement in a cell is a `.table-meter`: a 4rem track and its printed value.
- An order choice (`choice-group is-inline`) sits above the table, not in its header.
- A table wider than its card sits in `.table-scroll` (`overflow-x: auto`, `tabindex="0"`, a labelled region); it shows the focus ring.
