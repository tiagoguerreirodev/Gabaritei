---
name: add-command
description: Add a new Tauri command end to end (domain function + test, thin wrapper, registration, api.ts, regenerated bindings). Use whenever the frontend needs a new backend operation.
---

# Add a Tauri command

Follow AGENTS.md "Typed frontend contract". Do the steps in order.

## 1. Domain function and test

- Put the logic in its domain module (`bank`, `attempts`, `metrics`, `learning`, `ai`, `backup`, `migrate`), as plain Rust that takes a `&Connection` (or `&Database`) and returns `AppResult<T>`. No Tauri types here.
- Errors the user can act on are `AppError::Invalid` with a **pt-BR** message (e.g. `AppError::Invalid("Caderno não encontrado.".into())`). Unexpected failures are `AppError::Internal(detail)`; the user sees a generic message.
- Unit-test it against `Database::open_in_memory()` with small synthetic fixtures built in code. Never use real study data.
- **No spoilers:** if the result can contain a question, `answer_id` and explanation must be absent until that question has been answered. Enforce it in this function, and test it.

## 2. Types that cross IPC

Every input and output struct/enum:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Thing { /* ... */ }
```

## 3. Thin wrapper in `src-tauri/src/commands.rs`

```rust
/// One-line doc in English.
#[tauri::command]
pub fn thing(db: State<'_, Database>, id: String) -> AppResult<Thing> {
    db.with(|conn| domain::thing(conn, &id))
}
```

- Only decode, call, encode. Use `db.transaction(...)` for writes.
- Blocking or slow work (file dialogs, imports, AI runs) goes in an `async` command with `tauri::async_runtime::spawn_blocking` (see `import_legacy_bank`).
- Register it in `tauri::generate_handler![...]` inside `register()` in `src-tauri/src/lib.rs`.
- Add an IPC test in the `commands.rs` test module with `invoke_with(...)`, checking camelCase argument decoding and the `{ "message": ... }` error shape.

## 4. Frontend function in `src/api.ts`

```ts
/** One-line doc. */
thing: (id: string) => call<Thing>("thing", { id }),
```

- Import the type from `./bindings/Thing`. Argument keys are camelCase.
- Never call `invoke` anywhere else.
- Add a case to `src/api.test.ts` with `mockIPC` when the call has arguments or special handling.

## 5. Regenerate and verify

```sh
cargo test --manifest-path src-tauri/Cargo.toml   # writes src/bindings/
npm run typecheck && npm test
```

Commit the regenerated `src/bindings/` files with the change. Never edit them by hand; CI fails if they're stale.
