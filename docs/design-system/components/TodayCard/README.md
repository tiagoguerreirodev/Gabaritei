# TodayCard
The first card on Início: the one thing to do today, with one primary button. Built on `card card-wide`.

- `section.card.card-wide.today-card` > `p.today-kicker` (why this, in `accent`, `small`, 600) > `h2` (what: a topic, an attempt title) > `p.muted` (one line of context) > `btn btn-primary`.
- It carries the Sheet's print edge: a 3px `accent` top border. That edge is the only decoration, and only this card and the Sheet have it.
- The action is decided by the backend's day summary, in this order: an unfinished attempt (*Continuar*), due reviews (*Revisar agora*), a weak topic (*Treinar*), else *Começar no Banco*. Every button but *Continuar* is a link to Banco with filters set.
- Never two primaries on Início: the cards below use default buttons and links.
