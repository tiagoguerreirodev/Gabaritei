# M8 · Clean code: execution plan

Milestone **M8 · Clean code** brings the codebase in line with the "Clean code" section of
[`AGENTS.md`](../AGENTS.md) without changing user-visible behavior. Each issue holds its own
scope, file pointers and acceptance criteria; read it with `gh issue view <n>` before starting.

## Schedule

1. Finish the open M4 and M6 issues first (#53, #54, #55, #81, #150, #151). They touch the AI
   runner and `set_folder`, which M8 refactors.
2. Then M8, in the order below.
3. Then M7 (release). M8 is numbered after M7 but runs before it.

## Order

Follow each issue's *Depends on* list. Issues on the same step can be done in any order.

| Step | Issue | Scope |
|---|---|---|
| 1 | #174 | ID newtypes and a typed attempt source reference |
| 2 | #175 | Enums stored through `ToSql`/`FromSql`, one parse convention |
| 3 | #176 | Core infrastructure: db settings, startup, error logging, shared lock |
| 4 | #177 | Split `attempts`; inject randomness |
| 5 | #178, #179, #180 | `metrics`, `learning`, `bank` |
| 6 | #181 | Split `backup.rs`; inject its dependencies (after #150, #151) |
| 7 | #182 | AI runner and jobs layers (after #53, #81) |
| 8 | #183 | Bug: recheck and explain source limits differ (needs an owner decision) |
| 9 | #184 | AI tasks: shared ask flow, typed states |
| 10 | #185 | `migrate` |
| 11 | #186 | Thin Tauri commands (the leftovers after the domain issues) |
| 12 | #188 | Move frontend business rules into Rust |
| 13 | #189 | Shared frontend building blocks |
| 14 | #190 | Split the large frontend components |

Bug #187 ("answered" is defined two ways) can be fixed at any point; it is related to #177.

## Rules for every M8 PR

- **No user-visible behavior change.** A bug found while refactoring gets its own `fix:` issue,
  not a hidden fix inside the refactor PR.
- **No schema change.** Enums keep their stored strings. If a migration is really needed, it is
  a new numbered file.
- **Bindings.** Newtypes are `#[serde(transparent)]`, so the IPC shape stays the same. Commit
  the regenerated `src/bindings/`.
- **Moves before edits.** When splitting a file, make a move-only commit first, then the
  behavior-preserving edits.
- **Tests.** Never weaken existing tests. Split multi-behavior tests (`…_and_…`) only in the
  modules the issue touches, one behavior per test, preferring whole-struct `assert_eq!`.
- **Docs.** Update the "Stack and layout" table in `AGENTS.md` when a module is split.
- **Extra checks:**
  - `spoiler-auditor` for #177, #184 and #188;
  - a manual `ci-macos.yml` run for #181, #182 and #184;
  - the ignored `real_legacy_bank` test for #180 and #185.
- **Workflow** (from `AGENTS.md`): branch `issue-<n>-<slug>`, one issue per PR, `Closes #<n>`,
  and all Rust and frontend checks green before opening the PR.
