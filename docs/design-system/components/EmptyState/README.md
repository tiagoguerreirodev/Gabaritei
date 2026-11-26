# EmptyState
What to show when a list or result is empty: one line saying what is missing and the one way forward. Never a blank area.

- `div.empty-state` (dashed `border-strong` edge) > `p.empty-state-title`, an optional explanatory `p`, then one link or button.
- Examples: Histórico with no attempts (*Nenhuma tentativa ainda.* → *Ir para o Banco*); Banco filters that match nothing (*Nenhuma questão com estes filtros.* → *Limpar filtros*).
- Loading failures are not empty states: use the error state with *Tentar de novo*.
