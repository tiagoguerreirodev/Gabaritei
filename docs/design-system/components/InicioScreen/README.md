# InicioScreen
Início, the day's dashboard: one TodayCard with the main action, a summary strip of figures, then quiet cards for what waits and how each area stands.

Composition: `h1` *Início*; `.today-card`; `ul.summary-strip` (*Respondidas hoje*, *Acerto na semana* with an accuracy badge, *Revisões pendentes*); a card *Aguardando você* (waiting packages, an `item-list`) only while something waits; *Aproveitamento por área* (`item-list` of Meters); *Outras tentativas em andamento* (`item-list is-ruled`). Before the first finished attempt, *Primeiros passos* replaces the figures: a checklist with ✓ marks for what is done.
