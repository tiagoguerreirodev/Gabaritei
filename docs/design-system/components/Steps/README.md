# Steps
A short run of numbered stages: the steps of the Gerar form and the stages of a running generation. `ol.steps`, each `li` with a counter drawn as an answer-sheet bubble.

- To do: an empty ring (`border-strong`) with its number. Done (`li.done`): a `success` ring with ✓. Current (`li.current`): a filled `accent` bubble and a bold label, plus `aria-current="step"`.
- The state is in the marker's shape and the label's weight, not only in colour, and a visually hidden word repeats it for screen readers (*concluída*, *em andamento*).
- Steps are a real sequence: use them only where order matters. A stage that may be skipped says so in a muted note under the list.
