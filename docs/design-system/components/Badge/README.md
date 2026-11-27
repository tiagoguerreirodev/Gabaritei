# Badge
A small pill label for status, tags, counts and accuracy, set in `caption` with tabular numerals.

- Default: neutral metadata (*Cebraspe*, *Simulado*), `border-strong` edge and `muted` text. Caderno meta in Banco uses it too, in sentence case.
- `badge-accent`: tags on collections and topics (`accent` on `accent-soft`).
- `badge-danger`: problems the user must look at (*Contestada*). Always a word, not just the colour.
- `badge-success`, `badge-dashed`: blind-check verdicts in Revisar pacote (*Confirmada*, *Não verificada*).
- `badge-count`: a number on a nav item (packages waiting) or a Banco preset (due reviews, errors). Hide it at zero.
- **Accuracy bands** `badge-good` (≥ 70%, `success`), `badge-fair` (50–69%, `flag`), `badge-poor` (< 50%, `danger`): the printed percentage is the meaning, the band only reinforces it. Used on Início's summary strip and weak-topic rows; Histórico scores stay plain text.
