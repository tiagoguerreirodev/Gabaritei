# Meter
A horizontal bar for mastery and progress: name, `pill` track with `border-strong` edge, `accent` fill, and the printed value. The number is required; the bar alone never carries the meaning.

- Full (`div.meter`): name, track, value in three columns. Per-area achievement on Início, progress on an attempt.
- Compact (`span.meter.compact`): track and value only, for a row that already shows the name (Banco's topic tree).
- In a table cell use `.table-meter` (DataTable); in the lead stat tile, a bare `.meter-track` at 0.5rem.
- Below `breakpoint-compact`, and at text size xl at any width, the name moves above the bar.
- The track is `role="meter"` with `aria-valuenow` and a label (*Aproveitamento em Direito*). Nothing evaluated yet: an empty track and *—*. The fill animates width at `dur-base`.
