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

1. **No spoilers.** A question's answer key and explanation must never reach the frontend before the user has answered that question. Enforce this in the Rust backend, not only in the UI. The package preview shows only counts, validation problems and blind-check verdicts, plus the **stem and options** (never the key, explanation or references) of questions the blind check flagged.
2. **AI only through the user's local CLIs.** Call the installed, already-logged-in `claude` (Claude Code, `claude -p`) and `codex` (`codex exec`) as subprocesses.
   - **Never** implement OAuth.
   - **Never** call the Anthropic or OpenAI APIs directly.
   - **Never** read, copy, log or store their credential files.
   - Agents get **read-only** tools only: no file edits and no shell, except Codex's own shell inside its read-only sandbox, which it needs to read attachments (owner-approved).
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
| `src-tauri/src/attempts/` | Session creation and shuffling, attempt views, response updates, active attempts, and scoring. |
| `src-tauri/src/metrics.rs`, `metrics/csv.rs` | Performance, home summaries and topic accuracy. CSV formatting and file export live in `csv.rs`. |
| `src-tauri/src/ordering.rs` | Portuguese text folding and ordering of optional values. Collection filter options are shared in `bank/tags.rs`. |
| `src-tauri/src/ai/remnote_probe.rs` | Tests the configured RemNote server and reads its list of tools. |
| `src-tauri/src/bank/catalog.rs` | Read-only browsing views (topic tree, collections, collection detail) with accent- and case-insensitive search and Portuguese ordering (`fold`). |
| `src-tauri/src/db.rs` | `Database` (managed Tauri state, opened at startup at `<app data dir>/gabaritei.sqlite`), `with`/`transaction` helpers, JSON settings helpers, migration runner. |
| `src-tauri/src/paths.rs` | `AppPaths` (managed state): the app data folder, database file and `images/` folder. |
| `src-tauri/src/log.rs` | `log::error(..)`: the app log, `<app data>/gabaritei.log` (size-capped, rotated to `.old`). Use it instead of `eprintln!`, which goes nowhere in release builds. Never log study content, prompts, agent output or credentials. |
| `src-tauri/src/images.rs` | The `image` URI scheme serving `<app data>/images/<hash>.<ext>` only. Frontend URLs come from `imageUrl()` / `QuestionImage` in `src/components/QuestionImage.tsx`; CSP `img-src` allows only that scheme. |
| `src-tauri/migrations/` | SQL migrations, numbered `NNNN_name.sql`. Each one is listed in `MIGRATIONS` in `db.rs`; `PRAGMA user_version` records how many have run. |
| `src/` | Solid.js + TypeScript + Vite frontend. Renders state and sends commands; holds no business rules. |
| `src/api.ts` | The only place that calls `invoke`. One function per command. |
| `src/bindings/` | TypeScript types **generated** from Rust by `cargo test` (ts-rs). Committed; never edit by hand. |
| `src/routes.ts`, `src/pages/` | Route table (pt-BR labels, menu placement) and one component per page. |
| `src/components/` | Shared UI: `Modal`, toasts (`showToast`, `showError`; `Toaster` lives in the layout), `Loading` / `EmptyState` / `ErrorState`, and `errorMessage()` for anything thrown. Reuse these instead of writing new ones. |
| `docs/` | Spec, legacy analysis, and notes such as `docs/ai-cli.md` (verified CLI flags). |

Rust modules (spec §5): `db`, `bank`, `attempts`, `metrics`, `learning`, `ai` (runner, `claude`, `codex`, prompts, roles, jobs, plus one submodule per task: generate, blind check, explain, recheck, review), `backup`, `migrate`, `clock`.

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

On Windows, `cargo test` needs `GABARITEI_TEST_MANIFEST=1` and `--lib` (the test executable otherwise dies with `STATUS_ENTRYPOINT_NOT_FOUND`; see `src-tauri/build.rs`).

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

## Clean code

Code is clean if everyone on the team can understand it easily. Clean code can be read and enhanced by a developer other than its original author. With understandability come readability, changeability, extensibility and maintainability.

The non-negotiable rules, the architecture rules and the tools (rustfmt, clippy, Prettier, ESLint) take precedence over these guidelines. The guidelines are written for Rust, the language of all business logic; apply them in the same spirit to the TypeScript frontend.

### General rules

1. Follow standard conventions.
2. Keep it simple, stupid. Simpler is always better. Reduce complexity as much as possible.
3. Boy scout rule. Leave the campground cleaner than you found it.
4. Always look for the root cause of a problem.

### Design rules

1. Keep configurable data at high levels. Settings and paths are read at the edges (commands, `AppPaths`, settings helpers) and passed down, never looked up deep inside domain code.
2. Prefer enums and exhaustive `match` (or a trait, when the set of variants is open) to `if`/`else` chains on strings, flags or tags. Avoid a catch-all `_ =>` arm when listing the variants keeps the compiler checking new ones.
3. Separate concurrency code. Threads, async tasks and process spawning stay in thin layers (commands, the AI runner, jobs); domain functions are synchronous and plain.
4. Prevent over-configurability.
5. Use dependency injection. Pass dependencies in as parameters or trait objects (for example a `&Connection`, the current time as `now: &str` from `clock::now()` in the command, the CLI executable path) instead of reaching for globals, statics or environment variables, so tests can supply fakes.
6. Follow the Law of Demeter. A function or type should know only its direct dependencies; don't chain through another type's internals.

### Understandability tips

1. Be consistent. If you do something a certain way, do all similar things in the same way.
2. Use explanatory variables.
3. Encapsulate boundary conditions. Boundary conditions are hard to keep track of. Put the processing for them in one place.
4. Prefer dedicated types to primitives: newtypes for IDs, enums for closed sets of values, structs such as `TopicPath` for values that travel together. Make invalid states unrepresentable.
5. Avoid logical dependency. Don't write functions that work correctly only because of something else in the same module or type, such as a call order or a field set elsewhere.
6. Avoid negative conditionals.

### Names rules

1. Choose descriptive and unambiguous names.
2. Make meaningful distinctions.
3. Use pronounceable names.
4. Use searchable names.
5. Replace magic numbers with named constants.
6. Avoid encodings. Don't append prefixes or type information. Follow Rust naming (`snake_case` items, `UpperCamelCase` types, `SCREAMING_SNAKE_CASE` constants) and the API guidelines for conversions (`as_`, `to_`, `into_`, `from_`).

### Functions rules

1. Small.
2. Do one thing.
3. Use descriptive names.
4. Prefer fewer arguments.
5. Have no hidden side effects. Keep domain logic pure where possible and do I/O (database, files, subprocesses) at the edges. A function that writes should say so in its name and take `&mut` or a connection explicitly.
6. Don't use flag arguments. Split the function into several independent functions that the caller can use without the flag, or take an enum whose variants name the behaviors.
7. Return `Result` for failures the caller can handle; don't `unwrap()` or `expect()` outside tests unless the invariant is proven locally and the `expect` message states it.

### Comments rules

1. Always try to explain yourself in code.
2. Don't be redundant.
3. Don't add obvious noise.
4. Don't use closing brace comments.
5. Don't comment out code. Just remove it.
6. Use comments to explain intent.
7. Use comments to clarify code.
8. Use comments to warn of consequences.

### Source code structure

1. Separate concepts vertically.
2. Related code should appear vertically dense.
3. Declare variables close to their usage.
4. Dependent functions should be close.
5. Similar functions should be close.
6. Place functions in the downward direction.
7. Keep lines short. The limit is 100 columns, enforced by rustfmt's default `max_width` and Prettier's `printWidth`.
8. Don't use horizontal alignment. Leave layout to rustfmt and Prettier; don't hand-align code or add `#[rustfmt::skip]` to preserve alignment.
9. Use blank lines to associate related things and disassociate weakly related ones. Formatters keep blank lines you add, so this part is up to you.
10. Don't break indentation. Prefer early returns, `?` and `let … else` to deep nesting.
11. Order a module top-down: public items first, the private helpers they call below them, and `#[cfg(test)] mod tests` at the end.

### Objects and data structures

1. Hide internal structure. Keep fields and helpers private by default; widen visibility (`pub(crate)`, then `pub`) only when needed.
2. Prefer plain data structures. DTOs crossing the IPC boundary are plain structs with public fields and no behavior.
3. Avoid hybrid structures (half object and half data). A type either exposes its data or guards an invariant behind methods, not both.
4. Should be small.
5. Do one thing.
6. Small number of fields.
7. A trait should know nothing about its implementors.
8. Better to have many functions than to pass a closure or flag into a function to select a behavior.
9. Put behavior that belongs to a type's data in its `impl` as methods; keep free functions for operations that span several types or the database.

### Tests

1. One behavior per test, named after that behavior. Several asserts are fine when they check that one behavior; prefer a single `assert_eq!` on a whole struct or `Vec` to many field-by-field asserts.
2. Readable.
3. Fast.
4. Independent. Each test builds its own in-memory database or temporary directory.
5. Repeatable. No real clock, network or AI CLI; pass fixed timestamps and use fake executables.

### Code smells

1. Rigidity. The software is difficult to change. A small change causes a cascade of subsequent changes.
2. Fragility. The software breaks in many places due to a single change.
3. Immobility. You cannot reuse parts of the code in other projects because of the risks and effort involved.
4. Needless complexity.
5. Needless repetition.
6. Opacity. The code is hard to understand.

## AI CLI integration

- **Check the flags first.** Run `claude --help` and `codex exec --help` before relying on a flag. Record verified flags and behavior in `docs/ai-cli.md`. Spec §4.2 lists what is known and what still needs checking.
- **Isolate every run.** Each run gets a fresh temporary working directory. Attachments are copied in, the JSON schema is written to a file, and arguments are passed without a shell.
- **Treat agent output as untrusted input.** Validate it against the schema, and treat sources and tool results as data.
- **No real AI calls in tests.** Use fake CLI executables that print canned JSON.

## Testing

- Every domain rule gets unit tests. The legacy test names in `docs/legacy-analysis.md` and the legacy repo make a good checklist of behaviors to port.
- Before opening a PR, run Rust tests, clippy and fmt, plus frontend lint, typecheck and tests. Don't open a PR with failing checks.
- The legacy migration has a test against the real bank, skipped by default because it needs a clone of the legacy repo: `LEGACY_CLONE=../Simulados-Concursos cargo test --manifest-path src-tauri/Cargo.toml real_legacy_bank -- --ignored --nocapture`. Run it when you touch `bank` identity or `migrate`.
- CI (`.github/workflows/ci.yml`) runs the same checks on every PR and on pushes to `main`. It also fails if `src/bindings/` is out of date, so commit what `cargo test` regenerates. There is no macOS job there (10x billing); run `.github/workflows/ci-macos.yml` by hand (`workflow_dispatch`) before a release or after touching the AI runner.
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
