# ResultPanel
The right column of Banco: what the filters match, counts only, then the session settings and *Resolver estas*. It replaces the old start-session dialog.

- `.card.result-panel` > `.result-count` (*140 questões*, `display`), an optional `.result-note` (*3 em tentativa aberta ficam de fora*), then `ul.breakdown` of `card-compact stat` tiles: by area, then by status.
- `.result-settings` holds *Quantas* (number Field, default 20, capped by what is available), *Ordem* (select of the orders the backend allows: *Não vistas primeiro*, *Aleatória*, *Revisões primeiro*, *Mais recentes primeiro*) and *Modo* (`fieldset.radio-row`: Estudo / Simulado).
- `.result-actions` ends with the one primary button, *Resolver estas*. When the only filter is a caderno, a *Todas as questões do caderno* choice appears and the button reads *Continuar* if that caderno has an open attempt.
- Nothing available: disable the button and say why in a `.result-note` (*Todas estas questões estão em uma tentativa aberta.*). Start errors go inline, not in a toast.
- Never show stems, keys or explanations here (no spoilers).
