# FilterChip
A removable filter value: the chosen caderno, or *Questões de: Revisões de hoje (24/11)* when Banco was opened from Histórico *Refazer*.

- `span.filter-chip` > `.filter-chip-text` (truncated with an ellipsis) and `button.filter-chip-remove` with an `aria-label` naming what it removes (*Remover caderno*).
- `accent` on `accent-soft`, pill-shaped. One chip per value; removing it clears that filter.
- Not a status badge: use Badge for read-only labels.
