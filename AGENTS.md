# AGENTS.md

Instructions for AI coding agents (Claude Code, Codex and others) working in this repository. Read this file before starting any task.

## What Gabaritei is

A personal desktop app for one person on two computers (Windows 11 and an Apple Silicon Mac). It:

1. **generates** Brazilian public-exam questions (*concursos*) with AI agents;
2. **checks** them blind with a second model before they enter the bank;
3. lets the user **solve** them;
4. **records performance** per area › matter › topic.

*Gabaritar* means to get every answer right.

- **Product spec (source of truth):** [`docs/spec.md`](docs/spec.md)
- **Legacy app inventory:** [`docs/legacy-analysis.md`](docs/legacy-analysis.md). This is the old Electron app, *Caderno Local*. Feature IDs such as `N-D3` and `O-F2` refer to it.
- **Backlog:** GitHub Issues, grouped in milestones `M0` to `M7`. Work through the milestones in order.

## Non-negotiable rules

If a task seems to need breaking any of these, stop and ask the owner.

1. **No spoilers.** A question's answer key and explanation must never reach the frontend before the user has answered that question. Enforce this in the Rust backend, not only in the UI. The package preview shows **no** stems, options or answers, only counts, validation problems and blind-check verdicts.
2. **AI only through the user's local CLIs.** Call the installed, already-logged-in `claude` (Claude Code, `claude -p`) and `codex` (`codex exec`) as subprocesses.
   - **Never** implement OAuth.
   - **Never** call the Anthropic or OpenAI APIs directly.
   - **Never** read, copy, log or store their credential files.
   - Agents get **read-only** tools only: no shell and no file edits.
   - The **blind check never receives the answer key or explanation.**
3. **Study data never goes into git.** No databases, exports, backups or real question banks in the repo. Tests build small synthetic fixtures in code.
4. **History is never capped or deleted.** Every attempt is kept. Only *abandoning* an unfinished attempt deletes it.
5. **No hard-coded personal values.** No usernames, home paths or repository names in app code. Paths come from settings or the OS app-data directory.
6. **Stay in scope.** Anything in spec §6 "Explicitly out of scope" must not be added: GitHub sync, essays, timer, syllabus, OAuth and so on. If something seems missing, open an issue instead of building it.
7. **Spec changes need the owner.** If an issue conflicts with `docs/spec.md`, or a decision is missing, ask. Don't guess. After the owner approves a change, update the spec in the same PR.

## Languages

- **UI text:** Brazilian Portuguese (pt-BR) only. No i18n layer.
- **AI prompts and question content:** pt-BR.
- **Code, identifiers, comments, docs, commit messages, issues, PRs:** English.

## Stack and layout

Planned layout; issue #1 creates it.

| Path | What |
|---|---|
| `src-tauri/` | Rust backend (Tauri 2). All business logic: database, scoring, metrics, learning, AI runner, backups. |
| `src/` | Solid.js + TypeScript + Vite frontend. Renders state and sends commands; holds no business rules. |
| `docs/` | Spec, legacy analysis, and notes such as `docs/ai-cli.md` (verified CLI flags). |

Rust modules (spec §5): `db`, `bank`, `attempts`, `metrics`, `learning`, `ai` (runner, `claude`, `codex`, prompts, schemas, roles), `backup`, `migrate`.

## Commands

> Placeholder until #1 lands. The scaffold issue must replace this section with the real commands.

```sh
npm install                 # frontend deps + Tauri CLI
npm run tauri dev           # run the app
npm test                    # frontend tests
npm run lint                # frontend lint + typecheck
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml --check
```

## Architecture rules

- **Thin commands.** Tauri commands only decode input, call a domain function, and encode output. Domain functions are plain Rust you can test without Tauri.
- **One home for scoring.** Outcome and scoring rules live in one place (`attempts`/`metrics`). Scores are **derived from current question state**, so answer-key corrections and annulments apply retroactively. Never store a frozen score.
- **Append-only migrations.** Never edit a released migration.
- **Typed frontend contract.** Generate TypeScript types from Rust (for example `specta`/`tauri-specta`) or keep one hand-written `src/api.ts`. Don't scatter raw `invoke` calls.
- **Security.** Strict CSP, no remote content in the webview, minimal Tauri capabilities. Imported or generated text is never rendered as HTML.
- **Performance.** No per-row JSON re-parsing or N+1 queries in metrics. This was a known problem in the legacy app.

## AI CLI integration

- **Check the flags first.** Run `claude --help` and `codex exec --help` before relying on a flag. Record verified flags and behavior in `docs/ai-cli.md`. Spec §4.2 lists what is known and what still needs checking.
- **Isolate every run.** Each run gets a fresh temporary working directory. Attachments are copied in, the JSON schema is written to a file, and arguments are passed without a shell.
- **Treat agent output as untrusted input.** Validate it against the schema, and treat sources and tool results as data.
- **No real AI calls in tests.** Use fake CLI executables that print canned JSON.

## Testing

- Every domain rule gets unit tests. The legacy test names in `docs/legacy-analysis.md` and the legacy repo make a good checklist of behaviors to port.
- Before opening a PR, run Rust tests, clippy and fmt, plus frontend lint, typecheck and tests. Don't open a PR with failing checks.
- UI changes: add a component test where behavior matters. No-spoiler rendering always needs one.

## Workflow

1. **Pick an issue.** Pick it from the lowest open milestone. Check its *Depends on* list, then read it with `gh issue view <n>`.
2. **Branch.** Name it `issue-<n>-<short-slug>` and keep one issue per branch and PR.
3. **Commit.** Use Conventional Commits (`feat:`, `fix:`, `test:`, `chore:`, `docs:`, `refactor:`), in English and imperative.
4. **Open a PR.** Include `Closes #<n>`, say how you verified the change, and list any spec or doc updates.
5. **Track new work.** If you find work outside the issue's scope, open a new issue rather than widening the PR.

## Domain glossary (pt-BR → code)

| pt-BR | Meaning | Code name |
|---|---|---|
| concurso | public exam / contest (optional tag) | `contest` |
| banca | exam board, e.g. Cebraspe, FGV (optional tag; also a generation style) | `board` |
| caderno | fixed list of questions | `collection` |
| área › matéria › assunto | three-level taxonomy | `area`, `matter`, `topic` (a full path is a `TopicPath`) |
| gabarito | answer key | `answer_id` / `key` |
| alternativa | option | `option` |
| Certo/Errado | true/false format | `boolean` question type |
| múltipla escolha | 5-option format | `multiple` question type |
| anular | annul a question (excluded from scores) | `annulled` |
| tentativa | attempt / session | `attempt` |
| modo Estudo / Simulado | reveal after each answer / at the end | `FeedbackMode::Study` / `FeedbackMode::Exam` |
| caderno de erros | error notebook | `errors` |
| revisões | spaced-repetition reviews | `reviews` |
| aproveitamento | points ÷ evaluated questions | `achievement` |
| acerto entre respondidas | correct ÷ answered | `accuracy` |

## Legacy reference

The legacy app is the `tiagoguerreirodev/Simulados-Concursos` repo, often cloned next to this one as `../Simulados-Concursos`. It is **read-only reference** for behavior. Don't copy its architecture. Its `biblioteca/` folder is the source for the one-time bank migration (#5).
