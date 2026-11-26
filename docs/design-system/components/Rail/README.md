# Rail
The question navigator as a sticky side rail beside the Sheet, laid out like the answer grid of a real *cartão-resposta*: progress meter on top, then groups of 20 bubbles in five columns (`bubble-lg`), then a legend.

- Each group has a `label` (*Q 1–20*). Bubbles use the Bubble states: `is-filled` answered, `is-current` on screen, `is-correct`/`is-wrong` with marks after feedback, a ⚑ mark when flagged.
- The legend spells out each state in words, so nothing depends on colour.
- Below 60rem the rail moves above the sheet as one horizontally scrolling row.
- Each bubble is a button with an `aria-label` (*Questão 4, respondida, marcada*); the rail is a `nav` labelled *Questões*.
