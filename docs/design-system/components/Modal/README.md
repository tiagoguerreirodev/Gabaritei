# Modal
A dialog over a `scrim` backdrop: `surface` box, `radius-md`, `shadow-overlay`, max 36rem wide. Header (`heading` title + ghost close button), body, footer with a `border` rule and right-aligned buttons.

- Use for confirmations and short forms only. The primary or danger button goes last (rightmost); *Cancelar* is a default button before it.
- Focus moves into the dialog on open and returns on close; Esc closes it.
- `.modal-backdrop` is fixed over the whole window (`z-index: 10`); a tall `.modal` stops at `calc(100vh - 4rem)` and scrolls inside.
