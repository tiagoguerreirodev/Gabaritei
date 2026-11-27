# StatGrid
A row of figure tiles for Desempenho and the verdict counts in Revisar pacote: `ul.stat-cards` of `li.card.card-compact.stat`.

- Each tile: `span.stat-name` (`small`, muted) then `span.stat-value` (`display`, tabular), and an optional `span.stat-hint` (`caption`) for the denominator (*de 418 avaliadas*).
- One tile may lead: `.stat-card-main`, two columns wide, `accent` edge on `accent-soft`, its value at `figure` (3rem) with a compact meter under it. Use it for *Aproveitamento* only. It drops to one column below 30rem.
- Tiles are at least `stat-tile-min` wide and fill the row; the grid caps at `content-width-wide`.
- A missing value prints *—*, never 0%.
