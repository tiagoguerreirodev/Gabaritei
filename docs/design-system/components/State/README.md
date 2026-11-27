# State
What a screen shows while it loads, when it has nothing, and when loading failed. `ResourceView` in `src/components/states.tsx` picks one.

- **Loading:** `div.state.skeleton` with two to four `.skeleton-line`s (`border` fill, `radius-sm`, a slow pulse that stops under reduced motion); `data-wide="true"` for a full-width line. A visually hidden *Carregando…* goes with it.
- **Empty:** `div.state.empty-state` > `p.empty-title` (what is missing, one line), an optional `p.muted`, and `div.state-action` with the one way forward. *Nenhuma tentativa ainda.* → *Ir para o Banco*; *Nenhuma questão com estes filtros.* → *Limpar filtros*.
- **Error:** `div.state.error-state` > `p.error` (the backend's pt-BR message, which may run several lines) and a default `btn` *Tentar de novo*.
- States sit where the content would be, plain on the page or inside its card: no extra frame, no illustration. An empty screen always names its next step.
