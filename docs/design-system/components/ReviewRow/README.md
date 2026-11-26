# ReviewRow
One generated question in Revisar pacote, as a full-width row in a list, filtered by verdict with `.filter` pills above.

- Row: `details.review-row` > `summary` with the number, the topic path (truncated), a verdict badge and a chevron. Verdicts: *Aprovada* (`badge-success`), *Contestada* (`badge-danger badge-dashed`), *Com problemas* (`badge-danger`), *Não verificada* (default badge). Dashed vs solid tells disputed from problems without colour.
- Expanding a flagged row shows the stem (`reading`) and options as a plain lettered list, plus the blind check's note in `danger`. **Never** the key, explanation or references (no spoilers).
- Filters: `.filter` buttons with `aria-pressed`, each with its count (*Todas 10*, *Contestadas 2*, *Com problemas 1*, *Aprovadas 7*). The active one is `accent-soft`.
- Import and discard actions stay at the bottom of the page as buttons (*Importar 9 questões* primary).
