# Shortcuts
The keyboard help in Resolver: a ghost toggle (*Atalhos*) that opens a raised panel of keys above it. `div.shortcuts` > `button.btn.btn-ghost[aria-expanded]` + `div.shortcuts-panel`.

- The panel is `surface` with a `border-strong` edge, `radius-md` and `shadow-raised`, in `small`. It floats, so it is one of the two raised things besides toasts.
- Keys are `kbd`: mono, `border-strong` hairline, `radius-sm`. List one action per line: <kbd>A</kbd>–<kbd>E</kbd> choose, <kbd>Enter</kbd> confirm, <kbd>←</kbd> <kbd>→</kbd> move, <kbd>M</kbd> flag.
- Escape and a click outside close it. Shortcuts can be turned off in Configurações; the panel then says so.
