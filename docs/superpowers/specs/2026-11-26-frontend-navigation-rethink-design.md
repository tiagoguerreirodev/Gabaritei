# Frontend navigation rethink: design

Status: draft for owner approval (2026-11-26). Needs spec changes (§3.1, §3.3, §3.4, §3.5 of
`docs/spec.md`) once approved.

## Intent

The owner finds the study flow awkward in three places:

1. **Choosing questions.** Questions can only be reached by walking area › matter › topic, and a
   session starts from exactly one topic, one collection, or the errors/reviews lists. The owner
   wants to **filter and combine** instead.
2. **Starting to study.** There are four doors into Resolver (Início, Banco tree, Banco
   collections, Desempenho's *Treinar*, Erros e revisões), each with its own start dialog.
3. **Finding past work.** A finished attempt is only reachable from a collection's detail or an
   error row. There is no history, and no way to redo a set.

The new design system (`docs/design-system/` on branch `claude/design-system-refactor-72b0fc`)
is being implemented at the same time. The owner chose to **rebuild each screen once**, applying
the new layout and the design system together.

## Goals

- **Banco is the one place to choose questions and start a session.** It is a filterable view of
  the bank, with a live count and *Resolver estas*.
- Every shortcut that starts a session lands in Banco **with the filters already set**; the user
  sees what will be solved, adjusts, and starts. One start flow instead of four dialogs.
- **Histórico** lists attempts (open and finished), opens a finished one to re-read it, and
  offers *Refazer*.
- The sidebar stays organized **by activity**.

## Non-goals

- Saving filter combinations, or saving a filtered set as a new caderno (owner: not now).
- Hand-picking individual questions into a session.
- Text search over stems.
- Changing Gerar, Revisar pacote, Resolver or Configurações flows. They get the design system
  restyle only (as already agreed in `docs/design-system/`).
- Anything in spec §6 (timer, exam mode, syllabus…).

## Anti-goals

- **Spoilers.** Banco and Histórico must never show a key or explanation for a question the user
  has not answered (rule 1). Banco shows no stems at all (see Banco below).
- Moving filtering or ordering rules into the frontend. Matching, counting and ordering are Rust.
- A second, competing way to start a session. If a screen wants "study this", it links to Banco.

## Navigation

Sidebar, top to bottom:

| Entry | Route | Notes |
|---|---|---|
| Início | `/` | Day dashboard (unchanged role). |
| Banco | `/banco` | Rebuilt: filters + start. Accepts a filter in the URL. |
| Gerar | `/gerar` | Unchanged flow. |
| Desempenho | `/desempenho` | Unchanged; *Treinar* links to Banco. |
| Histórico | `/historico` | **New.** |
| Revisar pacote | `/revisar-pacote` | Shown only while a draft waits (unchanged). |
| Configurações | `/configuracoes` | Footer (unchanged). |

**Erros e revisões (`/erros-e-revisoes`) is removed.** Its two lists become Banco status filters
and presets. The old route redirects to Banco with the *Revisões de hoje* preset, so a stale link
still lands somewhere sensible.

Resolver stays contextual (`menu: "none"`).

## Banco

### Layout

```
Banco                                              [Revisões de hoje] [Caderno de erros]
┌ Filtros ───────────────────┐  ┌──────────────────────────────────────────────────┐
│ ASSUNTOS                   │  │ 140 questões                                     │
│ ▸ Direito Administrativo ☑ │  │ Direito Administrativo · 96   Constitucional · 44 │
│ ▾ Direito Constitucional   │  │ Nunca resolvidas 61 · Erradas 22 · Vencidas 9    │
│     ☐ Controle de const.   │  │                                                  │
│     ☑ Direitos fundamentais│  │ Quantas  [20]   Ordem [Não vistas primeiro ▾]    │
│ STATUS                     │  │ Modo  (•) Estudo  ( ) Simulado                   │
│ ☐ Nunca resolvidas         │  │                                [Resolver estas]  │
│ ☑ Erradas  ☐ Em branco     │  └──────────────────────────────────────────────────┘
│ ☐ Revisão vencida          │
│ ☐ Sinalizadas              │
│ CADERNO                    │
│ [Todos os cadernos     ▾]  │
│ Limpar filtros             │
└────────────────────────────┘
```

- **Left: filters.** Three groups, combined with AND between groups and OR inside a group.
  - **Assuntos:** the area › matter › topic tree with a checkbox at every level and question
    counts. Checking an area or matter selects everything under it. Any mix is allowed.
  - **Status:** *Nunca resolvidas*, *Erradas*, *Em branco*, *Revisão vencida*, *Sinalizadas*.
    Definitions are the existing ones: *Erradas* and *Em branco* are the error-notebook rules of
    §3.4/§3.5 (latest *answered* response wrong; latest result blank); *Revisão vencida* is a due
    review; *Sinalizadas* means flagged in the question's latest attempt. Annulled questions are
    always excluded.
  - **Caderno:** one caderno from a searchable select (the existing contest/board filters of the
    Cadernos list move into this select's search), or *Todos*.
- **Right: the result.** The matching count, a breakdown by area and by status (counts only, no
  stems), then the session settings and *Resolver estas*.
- **Presets** at the top set filters and order in one click:
  - *Revisões de hoje*: status *Revisão vencida* + *Erradas*, order *Revisões primeiro*. This
    reproduces today's reviews session (due reviews first, then recent errors). The button shows
    the due count and the "N em tentativa aberta" note from §3.5.
  - *Caderno de erros*: status *Erradas*, order *Mais recentes primeiro*.

### Session settings

- **Quantas:** default 20, limited by the matching count and the existing `questionCountLimits`
  range.
- **Ordem:**
  - *Não vistas primeiro* (default): unseen first, then least recently answered (today's topic
    rule).
  - *Aleatória*.
  - *Revisões primeiro*: due reviews first, then recent errors (today's reviews rule). Offered
    only when *Revisão vencida* is checked.
  - *Mais recentes primeiro*: latest answered first (today's error list order). Offered only when
    *Erradas* or *Em branco* is checked.
- **Modo:** Estudo / Simulado, as today.
- Questions sitting in an unfinished attempt are left out of the session, as errors/reviews
  sessions do today; the result panel says how many were left out.

### A caderno on its own

When the only filter is a caderno and the user chooses *Todas* (the whole caderno), Banco starts
a **collection attempt** exactly as today: fixed list, one active attempt per collection, listed
in that caderno's history. If that caderno has an open attempt, the button reads *Continuar* and
resumes it. Any other combination starts a **filtered attempt**.

### URL state

Banco's filters live in the URL query (for example
`#/banco?assuntos=12,40&status=erradas&ordem=recentes`), so every shortcut is a plain link and the
Back button restores the filters. The URL holds IDs and enum names only; Rust validates them and
ignores unknown values.

## Histórico

```
Histórico                                    [Todas as origens ▾] [Estudo e Simulado ▾]
EM ANDAMENTO
  Direito Administrativo + 1 · Estudo · 8/20 respondidas · 25/11 14:02     [Continuar]
FINALIZADAS
  Caderno TRF5 2024 · Simulado · 34/40 · 72% · 24/11 21:10        [Abrir] [Refazer]
  Revisões de hoje · Estudo · 12/12 · 83% · 24/11 08:30           [Abrir] [Refazer]
```

- Open attempts on top, finished below, newest first. Each row: source label, mode, progress or
  score (achievement, from current question state, so key corrections apply), date.
- Filters: source kind (caderno, assuntos/filtro, revisões…) and mode.
- **Abrir** opens the finished attempt in Resolver, read-only, as error rows do today.
- **Refazer** opens Banco filtered to *the questions of that attempt* (a fourth filter group,
  shown as a removable chip "Questões da tentativa de 24/11"). The user can add a status (for
  example only *Erradas*) and start. A refazer of a caderno attempt with no other filter is a new
  collection attempt.
- Abandon stays in Resolver; abandoned attempts are deleted and never listed (rule 4).

## Other entry points

All of these become links to Banco with filters set:

| From | Today | New |
|---|---|---|
| Início main card: reviews | starts reviews session | Banco, *Revisões de hoje* preset |
| Início main card: weakest topic | starts topic session | Banco, that topic checked |
| Início main card: start | links to Banco | unchanged |
| Início main card: resume | opens attempt | **unchanged** (nothing to choose) |
| Desempenho *Treinar* | topic session dialog | Banco, that topic checked |
| Collection detail *Começar* | collection session | Banco, that caderno selected |
| Histórico *Refazer* | (new) | Banco, that attempt's questions |

`StartSessionModal` is deleted; Banco's result panel replaces it.

## Backend changes (Rust)

Owned by `bank`/`attempts`; frontend only renders.

1. **`QuestionFilter`** (new DTO): `topics: Vec<TopicNodeRef>` (area, matter or topic IDs),
   `statuses: Vec<QuestionStatus>`, `collection: Option<CollectionId>`,
   `attempt: Option<AttemptId>`.
2. **`bank_filter_summary(filter, now)`** → count, per-area counts, per-status counts, number
   left out because they sit in an open attempt, and the tree's per-node counts for the current
   filter. One query set, no N+1 (architecture rule). Returns no stems, keys or explanations.
3. **`SessionSource::Filter { filter, count, order: SessionOrder }`** (new variant). The existing
   `topic`, `errors` and `reviews` variants become presets of it and are removed once the
   frontend no longer sends them; `collection` stays for the whole-caderno case.
4. **`attempt_history(query)`** → open and finished attempts with a source label, mode, progress
   and derived score. Needs the attempt's source stored; check whether the typed attempt source
   reference from #174 already records enough for a filtered attempt, else add a migration.
5. Spoiler audit (`spoiler-auditor`) on 2–4.

## Approaches considered

- **Screen by screen, layout and design system together (chosen).** Each screen is rebuilt once.
  Avoids styling Banco and Histórico twice.
- **Navigation first on today's styles, design system later.** Quicker to try, but Banco would be
  restyled right after it is built.
- **Design system first, then navigation.** Restyles the tree and Erros e revisões, which this
  design then deletes.

For navigation: owner chose **by activity** over content-first (the taxonomy as the spine) and a
single *Estudar* hub.

## Order of work

Each step is one issue and one PR, behavior-preserving outside its screen.

1. **Foundations:** design-system tokens (`tokens.json` → generated `tokens.css`) and
   the shared component families of `bundle.css` (base, Button, Card, Badge, Field, Tabs, Meter,
   Modal, Toast, Nav) applied app-wide, and the shell restyled. No route changes: those land with
   their screens (steps 4 and 5).
2. **Banco backend:** `QuestionFilter`, `bank_filter_summary`, `SessionSource::Filter`, tests.
3. **Banco screen:** filters, result panel, presets, URL state; delete `StartSessionModal`.
4. **Entry points:** Início, Desempenho, collection detail link to Banco; remove Erros e revisões
   and add the redirect.
5. **Histórico:** `attempt_history`, the screen, *Refazer* via the attempt filter.
6. **Remaining screens with the design system:** Resolver, Gerar, Revisar pacote (redesigns
   already in `docs/design-system/`), Início, Desempenho, Configurações.

## Spec changes (same PR as step 2, after approval)

- §3.1: Banco is a filterable view (topics, status, caderno), no longer a tree browser.
- §3.3: a session starts from a filter (topics × status × caderno × past attempt) with count,
  order and mode, or from a whole caderno.
- §3.4 "Other views": per-session history moves to Histórico.
- §3.5: the Learning page is removed; error notebook and reviews become Banco status filters and
  presets with the same rules.
- New §3.x Histórico.

## Calls made without the owner (check these)

- Banco shows counts only, never stems (owner confirmed).
- Resuming an open attempt still goes straight to Resolver.
- Groups combine with AND, choices inside a group with OR.
- *Sinalizadas* means flagged in the question's latest attempt.
- A whole caderno alone still starts a collection attempt (keeps one active attempt per caderno).
- *Refazer* is a fourth filter ("questions of this attempt"), so it composes with status.
- The old `/erros-e-revisoes` route redirects to Banco's *Revisões de hoje*.
- The `topic`, `errors` and `reviews` session sources are folded into `Filter`.

## Left to the builder

- Wrapper component names and folder layout for the design-system classes.
- Exact copy of labels and empty states (pt-BR, following `docs/design-system/README.md`
  Content rules).
- Banco layout below 60rem (filters collapse above the result).
- Query-string format details.
