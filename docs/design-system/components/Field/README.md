# Field
A labelled input with the label above it: `.field` > `.field-label`, `.input` (input, select or textarea, full width), optional `.field-hint`, then `.field-error`.

- Labels sit above fields, never in a left column. Group related fields in a Form section; put short fields side by side in a `.form-row`.
- Fields in a row align to the top: labels share a line and boxes share a top edge, whatever hint or error sits below. Every `.input` (text, number, select) is at least 2.5rem tall, so boxes side by side match. Keep labels in a row to one line.
- The label is always visible; placeholders are examples, not labels. Mark optional fields "(opcional)" in `muted` rather than starring required ones.
- The input edge is `border-strong` (3:1). On error set `aria-invalid="true"` (edge turns `danger`) and link the message with `aria-describedby`.
- Errors say what to do, in pt-BR: *Informe a quantidade (1 a 50).*
- Checkboxes and radios stay native with `accent-color: var(--accent)`.
