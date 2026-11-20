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
  [--append-system-prompt "<role instructions>"]            # prompt on stdin
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
- **Models:** `codex debug models` lists them. The app defaults to **`gpt-6.2-sol`** with **`xhigh`**.
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
