# Sheet
The question surface in Resolver: a printed answer-sheet page instead of a boxed card. A `surface` column (`sheet-width`) with a 3px `accent` top edge, a numbered gutter (`gutter-width`) ruled off in `rule`, a `label` header and the stem in `reading-lg`.

- Structure: `.sheet` > `.sheet-header` (two `label`s: *Questão 12 · Direito Administrativo* in `accent`, then the board/format in `muted`, plus the flag ghost button), `.sheet-number` in the gutter beside the stem, `.sheet-stem`, `.options`, then `.sheet-footer` with *Anterior* / *Confirmar resposta* / *Próxima*.
- The sheet has no border or shadow: the top edge, the gutter rule and the `rule` lines are the only ornament. Never add more.
- Stem and options are capped at `measure` (65ch). The Explanation panel follows the options inside the same sheet.
- Sit it in `.solve-layout` with the navigator Rail to its right.
