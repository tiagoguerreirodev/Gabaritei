# Bubble
The answer-sheet mark: an empty `border-strong` ring that fills when chosen. It is the brand's signature shape, used inside Option (letter) and in the question navigator (`bubble-lg`, number).

- States: `is-filled` (answered or chosen, `accent`), `is-correct` / `is-wrong` (after feedback, `*-soft` fill), `is-key` (the right answer the user did not pick), `is-current` (the question on screen, 2px `text` outline), `is-annulled` (struck number).
- Outcome bubbles carry a `.bubble-mark` (✓ or ✗) so colour is never alone; flagged questions carry `.bubble-mark.is-flag` (⚑) on the top corner.
- Navigator bubbles are buttons with an `aria-label` such as *Questão 4, respondida, marcada*.
