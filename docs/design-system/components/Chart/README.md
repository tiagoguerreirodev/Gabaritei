# Chart
The weekly trend on Desempenho and the sparkline beside a weak topic: hand-drawn SVG, one series, one colour.

- `svg.trend-chart` (up to `chart-width`): `.grid` gridlines in `border`, `.axis` labels in muted at `axis` (11px), one `.line` in `accent` at 2px with `.dot`s at each week. No area fill, no legend: there is one series and the card's `h2` names it.
- `svg.sparkline`: 1.5px `accent` line and an end dot, inline on the row's baseline.
- A chart never stands alone: `p.chart-note` says the latest value in words (*Esta semana: 68%, 4 pontos acima da anterior.*), and the SVG has a `title` with the same sentence. Weeks with nothing answered are gaps, not zeros.
