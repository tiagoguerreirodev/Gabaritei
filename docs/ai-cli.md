# Local AI CLIs: verified flags

How Gabaritei runs `claude` and `codex` (spec §4.2, AGENTS.md rule 2). Everything here was checked
against the installed CLIs with real runs, not only `--help`.

- Verified on 2026-11-20 with **Claude Code 2.1.329** and **codex-cli 0.171.0**.
- The app never reads, copies, logs or stores the CLIs' credential files. Login state comes from
  each CLI's own status command.
- Tests never call a real CLI. They use fake executables (`src-tauri/src/ai/runner/tests.rs`).
- Code: `src-tauri/src/ai/` (`runner`, `claude`, `codex`, `discovery`).

## Common to both

| Topic | What we do |
|---|---|
| Spawning | No shell. `Command` with an argument list. Stdout and stderr are piped. |
| Prompt | Written to **stdin**, then stdin is closed. This keeps answer keys out of the process list and avoids argument length limits. |
| Working folder | A fresh temp folder per run (`gabaritei-run-*`), deleted afterwards. It holds `schema.json` and `anexos/` (attachments copied in). |
| API keys | `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `OPENAI_API_KEY` and `CODEX_API_KEY` are removed from the child's environment, so a CLI can't silently bill an API key instead of the user's login. |
| Cancel / timeout | Unix: `SIGINT` to the process group, a grace period (3 s), then `SIGKILL` to the group, always, to catch children. Windows: `taskkill /T /F`. Default timeout 20 min. |
| Progress | Only fixed pt-BR labels ("Lendo as fontes…"), never model text, so a generation can't leak a question through progress events. |
| Effort names | `low`, `medium`, `high`, `xhigh`, `max` work on both. |
| Discovery | Override from settings, then `PATH`, then `~/.local/bin`, `~/.claude/local`, npm, bun, volta, cargo, Windows npm/WinGet folders, `/opt/homebrew/bin`, `/usr/local/bin`. Finder-launched apps have a short `PATH`. |
| Version | `<cli> --version`. |
| Login state | `claude auth status` prints JSON with `loggedIn`. `codex login status` exits 0 only when logged in. |

## Claude Code (`claude -p`)

```
claude -p --output-format stream-json --verbose --json-schema '<schema>'
  --model <m> --effort <low|medium|high|xhigh|max>
  --setting-sources "" --strict-mcp-config
  --tools "Read,Grep,Glob[,WebSearch,WebFetch]"
  --allowedTools "Read(./**),Grep(./**),Glob(./**)[,WebSearch,WebFetch][,mcp__remnote__<tool>...]"
  --permission-mode dontAsk --no-session-persistence --disable-slash-commands
  [--mcp-config mcp.json]
  [--append-system-prompt-file <config folder>/instrucoes.md]  # role instructions; prompt on stdin
```

- **Structured output:** the last `{"type":"result"}` event has `structured_output` (the answer),
  `is_error`, `total_cost_usd` and `result` (the text). No `structured_output` means invalid.
- **Isolation (verified):** the `system`/`init` event lists `tools` as only `Glob`, `Grep`, `Read`
  and `StructuredOutput`, no user plugins (only built-in ones) and no MCP servers. `--setting-sources ""`
  stops `~/.claude` and the project's settings and hooks from loading. Run in the temp folder, so no
  project `CLAUDE.md` or `.mcp.json` is found.
- **Not `--bare`:** bare mode reads only `ANTHROPIC_API_KEY`, skipping the subscription login.
- **Events used:** `system`/`init`, `assistant` (`tool_use` names), `result`.
- **RemNote:** `mcp.json` with an `http` server and an `Authorization: Bearer` header, written (mode
  0600) in a **separate** temp folder, not under the working directory, so the agent's file tools
  (scoped to the run folder) can't read the token. Deleted after the run. The token is never an
  argument.
- **File scope (verified):** `Read(./**)` lets the agent read inside the run folder only. A read of a
  file outside it is denied (`permission_denials` lists it).
- **RemNote tools are an allowlist:** only the exact tool names the user approved in Configurações
  are allowed (`mcp__remnote__<tool>`), never the whole server and never a name-pattern denylist.
  With no approved tools the agent gets no RemNote tools.

## Codex (`codex exec`)

```
codex exec - --output-schema schema.json -o saida.json --json
  --sandbox read-only --skip-git-repo-check --ephemeral --ignore-rules --strict-config
  -m <model> -c model_reasoning_effort=<e> -c web_search=<live|disabled>
  [--ignore-user-config] [--disable shell_tool --disable unified_exec]
  [-c mcp_servers.remnote.url="<url>" -c mcp_servers.remnote.bearer_token_env_var="GABARITEI_REMNOTE_TOKEN"
   -c mcp_servers.remnote.enabled_tools='["<tool>",...]']
```

- **Stdin matters:** `codex exec` prints "Reading additional input from stdin..." and **waits for
  stdin to close** even when a prompt is given. Always close stdin (we write the prompt and close).
- **Structured output:** `-o` writes the last message, which is the schema-conforming JSON. The
  `Package` schema from `schemars` is accepted as is (verified with a real run).
- **Events (`--json`, JSONL):** `thread.started`, `turn.started`, `item.started`/`item.completed`
  (`item.type`: `agent_message`, `reasoning`, `command_execution`, `web_search`, `mcp_tool_call`),
  `turn.completed` (with `usage`), `turn.failed` and `error` (with a message). Codex reports no cost.
- **Models:** `codex debug models` prints `{"models":[...]}` (it does not accept `--strict-config`). Per
  model the app reads `slug` (the `-m` value), `display_name`, `visibility` (`list` or `hide`; hidden
  ones are not offered), `default_reasoning_level`, `supported_reasoning_levels[].effort` and
  `service_tiers[]`. The rest, including long prompt text, is ignored. Effort names seen: `low`,
  `medium`, `high`, `xhigh`, `max` and `ultra` (Codex only). The app defaults to **`gpt-6.2-sol`**
  with **`xhigh`**.
- **Fast Mode (Codex):** every model reports a `priority` tier named "Fast"; a non-empty
  `service_tiers` means the model supports it. `-c service_tier="priority"` is accepted by
  `codex exec --strict-config` and a made-up key is rejected, but the value is not validated and
  the events do not show which tier served a request, so **the effect is unobserved**.
- **Fast Mode (Claude):** `--settings '{"fastMode":true}'` turns it on with `--setting-sources ""`
  in place (`system`/`init` `fast_mode_state` becomes `on` for Opus and stays `off` for Sonnet).
  Without extra usage on the account the `result` event reports `off` with
  `fast_mode_disabled_reason: extra_usage_disabled`. Claude has no command that lists models.
  See `docs/superpowers/specs/2026-11-21-catalog-spike-findings.md`.
- **Web search:** `-c web_search=live` or `disabled`, as the generation form asks. It can be
  combined with attachments and RemNote (owner decision, 2026-11-20: nothing in the notes is
  secret).
- **`--strict-config`:** makes Codex fail on an unknown `-c` key instead of ignoring it (verified:
  `mcp_servers.<name>.enabled_tools` is accepted, a made-up key is rejected). The RemNote tool
  allowlist uses `enabled_tools`.
- **"Use my Codex config" cannot be restricted by the app:** the user's own RemNote server may
  expose write tools. Configurações warns about it.
- **User config:** `--ignore-user-config` skips `~/.codex/config.toml` (hooks, MCP servers,
  instructions); auth still comes from `CODEX_HOME`. It is omitted only when the user chose
  "use my Codex config" for RemNote.
- **Shell (owner-approved, 2026-11-20):** Codex reads files through its shell, and `--disable shell_tool
  --disable unified_exec` leaves it unable to read attachments (verified: it answered "can't read").
  So the shell is disabled when a run has no attachments, and kept, inside `--sandbox read-only`
  (no writes), when it has. Codex has no other way to read attachments.
- **What the read-only sandbox can read (investigated for #53, codex-cli 0.171.0, Linux):**
  `--sandbox read-only` lets the shell read **any** file the user can read. Verified: with the
  run folder as cwd, `cat <absolute path outside the run folder>` returned the contents of a canary
  file. So a prompt-injected page or attachment could make Codex read, say, `~/.ssh`, and the
  content would reach the model provider's context, and from there web-search queries. Both
  CLIs' own credential files are readable too (a worse case than `~/.ssh`, and it would break
  AGENTS.md rule 2).
- **Narrowing it works with a permissions profile (verified on Linux):** replacing `--sandbox` with
  `-c default_permissions="gab" -c 'permissions.gab.filesystem={":minimal"="read", "<codex install dir>"="read", ":workspace_roots"={"."="read"}}'`
  (run folder as cwd) let `cat anexos/a.md` work and made the same canary file outside the run
  folder invisible ("No such file"). Caveats:
  - `:minimal` alone is not enough: the sandbox re-executes the `codex` binary, so its install
    folder (`~/.codex/packages/...` for the standalone install) must also be readable, or every
    command fails with `bwrap: execvp ...: No such file`. The app would have to work that folder
    out from the resolved CLI path, and the layout differs per install method and OS.
  - **Never grant `~/.codex` itself:** it holds `auth.json`. Only the install subfolder
    (`packages/...`).
  - `:minimal` is "platform defaults" (system folders needed to run commands); the exact list was
    not inspected, and only one outside canary path was tested, so "hides everything else" is
    not proven.
  - Not checked: whether writes and network stay blocked under the profile, and whether it works
    together with `--strict-config` (the verification run did not pass `--strict-config`).
  - The `":workspace_roots"` key only parses inside one inline table (`-c 'a.b.":x"=...'` keeps
    the quotes and is rejected).
  - Not verified on macOS (Seatbelt) or on Windows, where the sandbox has its own backends. Codex's
    docs say unsupported split policies fail closed on Windows (not tested here).
- **Other options, not implemented:** inlining MD/TXT attachments into the prompt would let those
  runs keep the shell disabled (only PDFs would need it), but large files need a size cap and the
  prompt-injection surface of the attachment text stays. Codex has no domain allowlist for web
  search (the app only uses `web_search=live` or `disabled`; other modes were not checked).
- **RemNote token:** passed in the child's environment (`GABARITEI_REMNOTE_TOKEN`), never as an argument.

## Windows (not verified)

Nothing here was run on Windows yet. Known risks to check on the Windows machine: npm installs give
`claude.cmd` / `codex.cmd`, and Rust's batch-file argument escaping can refuse some arguments (the
discovery order tries `.exe` first); `cmd.exe` limits a command line to 8191 characters, and Claude
gets the JSON schema and the role instructions as arguments; Codex's read-only sandbox on Windows
is untested; `taskkill /T /F` is the only cancel path (no SIGINT).

## Failures

- A login problem shows up as a non-zero exit with a message such as "Not logged in" or an
  `error`/`turn.failed` event (Codex), or an `is_error` result (Claude). The runner maps those to
  `NotLoggedIn`; any other non-zero exit becomes `Failed` with the exit code and the last error line.
- The CLI's own error text is **never** shown to the user (after a generation it could contain a
  question or its key); only the exit code and a fixed hint for known cases (usage limit,
  overloaded service) are kept.
- Models are passed as `--model=<m>` so a value can never be read as another flag.
- Not checked: the exact wording of every login failure (both CLIs were logged in during
  verification). The runner matches several phrases; tests cover them with fakes.

## Windows (issue #49)

Checked by the `rust-test-windows` CI job (`windows-latest`) with batch-file fake CLIs:

- **`.cmd` shims work.** A `claude.cmd` given the real argument list (JSON schema with quotes) and
  a codex `.cmd` run and return their answers.
- **Long, multi-line text never travels as an argument.** Rust refuses newlines in the arguments of
  batch files and `cmd.exe` caps a command line at 8191 characters. The role instructions went in
  a long `--append-system-prompt` argument, so they now go through
  `--append-system-prompt-file <config folder>/instrucoes.md` (a temp folder outside the run folder, so the agent's file tools can't list it) (verified with the real `claude` 2.1.329: the file is honored in `-p` mode).
  The prompt is on stdin; the schema is the only large argument left (compact JSON, no newlines;
  a test keeps all arguments under 4,000 characters in total).
- **Timeout and cancel end the whole tree.** `taskkill /PID <pid> /T /F` removes a child started
  with `start /b` as well as the batch process.
- **Discovery** tries `<name>.exe` then `<name>.cmd` in each folder.
- CI runs `cargo test --lib` only, with `GABARITEI_TEST_MANIFEST=1` (see `src-tauri/build.rs`):
  without a Common Controls v6 manifest the Windows test executable dies at startup with
  `STATUS_ENTRYPOINT_NOT_FOUND`.

Still to check by hand on the Windows 11 machine, with the real CLIs:

- Codex's read-only sandbox on Windows (can it read `anexos/`?).
- A real `claude` / `codex` run end to end from the installed app (login probe, generation, blind check).
