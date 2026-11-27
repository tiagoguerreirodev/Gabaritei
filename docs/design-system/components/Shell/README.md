# Shell
The frame every screen sits in: the Nav column, then a content column that holds one `.page`.

- `div.shell` > `aside.nav` + `main.content#main` > `div.page` > `h1`, then cards. The page title is the only `h1` (`title`, 1.5rem); every card and dialog title is an `h2` (`heading`).
- Cards on a page stack `space-6` apart and cap at `content-width` (`card-wide` for lists, tables and stat grids). Content is left-aligned; only Resolver centres its sheet.
- Content padding is `space-8` × `space-10`, and `space-6` × `space-5` below `breakpoint-narrow`.
- **Focus mode:** while an unfinished attempt is open in Resolver, `.shell.focus-mode` drops the nav column so the page is only the exam. Leaving the attempt brings it back.
- The first tab stop is `.skip-link` (*Ir para o conteúdo*), hidden until focused. `main` takes focus on route change and shows no ring.
- Toasts mount once in the shell, fixed bottom-right (`space-5` from the edges).
