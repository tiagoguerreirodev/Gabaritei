# Toast
A short, transient message in the bottom-right corner: `surface`, `border`, `shadow-raised`, with a round `.toast-mark` that gives the status a shape.

- Info/success: `accent` mark with ✓. Error: `.toast-error`, `danger` mark with `!`, and the message stays until dismissed.
- One sentence in pt-BR; an optional ghost close button. Errors that block a form go inline as a Field error, not a toast.
- `.toasts` is the fixed stack that holds them (right and bottom `space-5`, `space-2` apart, `z-index: 20`); each `.toast` slides in with `toast-in` over `dur-base`, and shows at once under reduced motion.
