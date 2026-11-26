# HistoryRow
One attempt in Histórico: what it was, how it went, and what to do with it.

- `ul.history-list` (a bordered surface) of `li.history-row`: `.history-title` (the source label, *Caderno TRF5 2024*, *Direito Administrativo + 1*), `.history-meta` (mode · progress or score · date, tabular numerals) and `.history-actions`.
- **Em andamento**: progress *8/20 respondidas* and one `btn-primary` *Continuar*.
- **Finalizadas**: *34/40*, achievement in `.history-score` (*72%*, or *—* when every question was annulled), the finish date, then *Abrir* (read-only in Resolver) and *Refazer* (Banco with that attempt's questions), both default buttons.
- Scores are computed from the current key, so a key correction changes them. Rows never show stems or keys.
- Group rows under `.history-section` headings in `label` style (*Em andamento*, *Finalizadas*), newest first, with a Pager below when there is more than one page.
- Below 40rem the actions wrap under the text.
