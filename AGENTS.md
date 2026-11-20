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

| Path | What |
|---|---|
| `src-tauri/` | Rust backend (Tauri 2). All business logic: database, scoring, metrics, learning, AI runner, backups. |
| `src-tauri/src/commands.rs` | Tauri commands (thin wrappers). Register each one in `register()` in `lib.rs`. |
| `src-tauri/src/error.rs` | `AppError`: every command returns `AppResult<T>`; the frontend receives `{ message }` in pt-BR. |
| `src-tauri/src/db.rs` | `Database` (managed Tauri state, opened at startup at `<app data dir>/gabaritei.sqlite`), `with`/`transaction` helpers, JSON settings helpers, migration runner. |
| `src-tauri/src/paths.rs` | `AppPaths` (managed state): the app data folder, database file and `images/` folder. |
| `src-tauri/migrations/` | SQL migrations, numbered `NNNN_name.sql`. Each one is listed in `MIGRATIONS` in `db.rs`; `PRAGMA user_version` records how many have run. |
| `src/` | Solid.js + TypeScript + Vite frontend. Renders state and sends commands; holds no business rules. |
| `src/api.ts` | The only place that calls `invoke`. One function per command. |
| `src/bindings/` | TypeScript types **generated** from Rust by `cargo test` (ts-rs). Committed; never edit by hand. |
| `src/routes.ts`, `src/pages/` | Route table (pt-BR labels, menu placement) and one component per page. |
| `docs/` | Spec, legacy analysis, and notes such as `docs/ai-cli.md` (verified CLI flags). |

Rust modules (spec §5): `db`, `bank`, `attempts`, `metrics`, `learning`, `ai` (runner, `claude`, `codex`, prompts, schemas, roles), `backup`, `migrate`.

Frontend versions: **Solid 1.9** with `@solidjs/router` 0.15 (`HashRouter`). Don't upgrade to Solid 2 outside #26.

## Commands

Prerequisites: Rust stable (`rust-toolchain.toml`), Node 26 (`.nvmrc`), and the [Tauri 2 system dependencies](https://v2.tauri.app/start/prerequisites/). If `cargo` isn't found, add `~/.cargo/bin` to `PATH`.

```sh
npm install                 # frontend deps + Tauri CLI (run once, and after package.json changes)
npm run tauri dev           # run the app with hot reload (Vite + Rust)
npm run tauri build -- --debug --no-bundle   # build the app with the frontend embedded (production CSP)

npm test                    # frontend tests (Vitest + jsdom)
npm run lint                # ESLint (typescript-eslint + eslint-plugin-solid) and Prettier check
npm run typecheck           # tsc for the app and for vite.config.ts
npm run format              # Prettier write + ESLint autofix

cargo test --manifest-path src-tauri/Cargo.toml        # Rust tests; also regenerates src/bindings/
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml --check
```

## Architecture rules

- **Thin commands.** Tauri commands only decode input, call a domain function, and encode output. Domain functions are plain Rust you can test without Tauri.
- **One home for scoring.** Outcome and scoring rules live in one place (`attempts`/`metrics`). Scores are **derived from current question state**, so answer-key corrections and annulments apply retroactively. Never store a frozen score.
- **Append-only migrations.** Never edit, reorder or delete a migration that has been merged. To change the schema, add the next numbered file in `src-tauri/migrations/`, append it to `MIGRATIONS` in `db.rs`, and test the upgrade from the previous version. Tables are `STRICT`; timestamps are RFC 3339 UTC text; booleans are `0`/`1`; JSON columns are checked with `json_valid()`.
- **Typed frontend contract.** Types that cross the IPC boundary derive `ts_rs::TS` with `#[ts(export)]` (plus `#[serde(rename_all = "camelCase")]` for structs), and `cargo test` writes them to `src/bindings/`. Never call `invoke` outside `src/api.ts`. To add a command:
  1. write the domain function and test it;
  2. add a `#[tauri::command]` wrapper in `commands.rs` returning `AppResult<T>`, and register it in `register()` (`lib.rs`);
  3. add a function to `api` in `src/api.ts`;
  4. run `cargo test` and commit the regenerated `src/bindings/`.
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
- The legacy migration has a test against the real bank, skipped by default because it needs a clone of the legacy repo: `LEGACY_CLONE=../Simulados-Concursos cargo test --manifest-path src-tauri/Cargo.toml real_legacy_bank -- --ignored --nocapture`. Run it when you touch `bank` identity or `migrate`.
- CI (`.github/workflows/ci.yml`) runs the same checks on every PR and on pushes to `main`. It also fails if `src/bindings/` is out of date, so commit what `cargo test` regenerates.
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
| concurso | public exam / contest (optional tag on collections) | `contest` |
| banca | exam board, e.g. Cebraspe, FGV (optional tag on collections; also a generation style) | `board` |
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
