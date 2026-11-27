# Nav
The sidebar, organized by activity: a brand row with the collapse toggle, the main menu, then Configurações and the version in the footer.

- Order: **Início, Banco, Gerar**, then **Revisar pacote** only while a generated package waits (with a `badge-count`), then **Desempenho, Histórico**; **Configurações** in the footer (`nav aria-label="Menu secundário"`). Resolver has no entry: it opens from Banco, Início or Histórico.
- Structure: `aside.nav` > `div.nav-brand` (`span.nav-brand-name` + ghost toggle *Recolher menu*) > `div.nav-menu` > `nav` of `a.nav-item` (icon + `span.nav-label`) > `div.nav-footer` > `nav` + `p.nav-version`.
- The current page has `aria-current="page"`: `accent` text on `accent-soft`.
- Icon-only form (`.shell[data-nav="collapsed"]`): chosen with the toggle, and always below `breakpoint-narrow`. Width `sidebar-width-collapsed`; labels stay in the accessibility tree and show as a tooltip (`surface`, `shadow-raised`, `radius-sm`) on hover or focus; the count badge sits on the icon's corner.
- In focus mode (an unfinished attempt open) there is no nav at all; see Shell.
- The old *Erros e revisões* route redirects to Banco with *Revisões de hoje*.
