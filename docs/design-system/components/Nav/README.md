# Nav
The sidebar, organized by activity: brand name, then one `.nav-item` per page (icon + label), Configurações and the version at the bottom.

- Order: **Início, Banco, Gerar, Desempenho, Histórico**, then **Revisar pacote** only while a generated package waits (with a `badge-count` of waiting packages), and **Configurações** in the footer. Resolver has no entry: it opens from Banco, Início or Histórico.
- There is no *Erros e revisões* entry any more: its lists are Banco presets (*Revisões de hoje*, *Caderno de erros*). The old route redirects to Banco with *Revisões de hoje*.
- The current page has `aria-current="page"`: `accent` text on `accent-soft`.
- Below 60rem, or when the user collapses it, the nav shows icons only (`sidebar-width-collapsed`). Labels stay in the accessibility tree and show as a tooltip (`surface`, `shadow-raised`, `radius-sm`) on hover or focus. Write the collapsed rules once, keyed on the shell's collapsed state.
- Icons are the app's line set (`src/components/icons.tsx`), inline as SVG with `currentColor`. Histórico uses the clock-with-return-arrow glyph that *Erros e revisões* used.
