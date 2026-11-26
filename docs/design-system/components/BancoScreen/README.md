# BancoScreen
The rebuilt Banco: the one place to choose questions and start a session. Presets on top, FilterPanel on the left, ResultPanel on the right, all state in the URL.

Composition: `h1` *Banco* with `.bank-presets` (default buttons with `aria-pressed` when the current filters equal the preset, each with its `badge-count`), then `.bank-layout` > FilterPanel + ResultPanel. Below 60rem the filters stack above the result. Counts only: Banco never shows a stem.
