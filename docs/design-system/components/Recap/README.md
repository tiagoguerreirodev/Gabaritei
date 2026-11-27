# Recap
The result card shown in Resolver after *Finalizar*: the score in a sentence, the counts with their glyphs, then the facts. `section.card.recap`.

- `p.recap-headline` with the score in `strong` (1.5rem, tabular): *Aproveitamento de **70%** em 20 questões.*
- `ul.recap-counts`: `li.recap-correct` (✓ in `success`), `li.recap-wrong` (✗ in `danger`), `li.recap-blank` (— in muted), and `li.recap-annulled` struck through when any question was annulled.
- `dl.facts` in two columns: mode, time started and finished, caderno or filters.
- Then the actions: *Ver erros* (link to Banco with the attempt's errors) and *Refazer*. Scores are derived from the current question state, so a later annulment changes this card.
