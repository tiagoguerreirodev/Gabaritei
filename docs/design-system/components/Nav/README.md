# Nav
The sidebar: brand name, then one `.nav-item` per page (icon + label), optional `badge-count`, and the version at the bottom.

- The current page has `aria-current="page"`: `accent` text on `accent-soft`.
- Below 60rem, or when the user collapses it, the nav shows icons only (`sidebar-width-collapsed`). Labels stay in the accessibility tree and show as a tooltip (`surface`, `shadow-raised`, `radius-sm`) on hover or focus.
- Icons come from `assets/Icons`, inline as SVG with `currentColor`.
