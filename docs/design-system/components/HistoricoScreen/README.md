# HistoricoScreen
The Histórico page: open attempts first, finished ones below, newest first, with *Origem* and *Modo* filters and a Pager.

Composition: `h1` *Histórico* with two select Fields (*Origem*: Todas, Cadernos, Filtros do Banco, Anteriores; *Modo*: Estudo e Simulado, Estudo, Simulado), then `.history-section`s of HistoryRows, then `.pager`. With no attempts at all, an empty State that links to Banco.
