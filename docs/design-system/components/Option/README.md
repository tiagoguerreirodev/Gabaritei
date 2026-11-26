# Option
One row of the answer sheet: a full-width button holding a Bubble with the letter and the option text in `reading-lg` (18px serif).

- Rows sit `space-3` apart with `space-3` × `space-4` padding, so long options never run together; the bubble aligns with the first line.
- Before answering: plain (hover strengthens the edge to `border-strong`), or `is-selected` (`accent-soft` row, filled bubble). Never show `is-correct` before the user has answered (no spoilers).
- After feedback: the chosen-and-right row is `is-correct` with a filled `success` bubble; a wrong choice is `is-wrong`; the key the user missed is `is-correct` with an unfilled `is-key` bubble. Add an `.option-note` word (*Gabarito*, *Sua resposta*).
- `is-eliminated` strikes an option the user ruled out (a ghost button toggles it).
- Certo/Errado questions use two options, C and E. Place options inside a Sheet.
