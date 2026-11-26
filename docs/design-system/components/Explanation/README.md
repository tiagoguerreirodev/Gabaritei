# Explanation
The panel that appears under the options once the user has answered: an `explain-bg` block (`radius-md`, no border) with three labelled sections, *Gabarito*, *Explicação* and *Referências*.

- Render it only after the answer is recorded; the backend sends the key, explanation and references only then (no spoilers).
- *Gabarito* states the verdict in words and a glyph (`✓ Você acertou: C` in `success`, `✗ Você marcou B; gabarito C` in `danger`).
- *Explicação* is `reading` (16px serif, 1.7). *Referências* is `reading-sm` in `muted`, as a list.
- Actions (*Contestar gabarito*, *Pedir nova explicação*) are default buttons in `.explanation-actions` at the end.
