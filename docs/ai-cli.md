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
  --setting-sources "" --strict-mcp-config [--settings '{"fastMode":true}']
  --tools "Read,Grep,Glob[,WebSearch,WebFetch]"
  --allowedTools "Read(./**),Grep(./**),Glob(./**)[,WebSearch,WebFetch][,mcp__remnote__<tool>...]"
  --permission-mode dontAsk --no-session-persistence --disable-slash-commands
  [--mcp-config mcp.json]
  [--append-system-prompt-file <config folder>/instrucoes.md]  # role instructions; prompt on stdin
```

- **Structured output:** the last `{"type":"result"}` event has `structured_output` (the answer),
  `is_error`, `total_cost_usd` and `result` (the text). No `structured_output` means invalid.
- **Schema dialect:** Claude Code 2.1.331 rejects an explicit Draft 2020-12 declaration.
  The Claude runner omits the top-level `$schema` from its CLI argument. The same checker schema
  with its local `$defs` references returned structured output with this version. Codex's schema
  file keeps the declaration.
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
  [-c service_tier="priority"]
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
- **The model-list probe is not an isolated run:** `codex debug models` runs without a run folder, a
  prompt or attachments (stdin closed, no shell, output capped at 8 MiB, killed after 20 s). It reads no
  credential file and sends no prompt.
- **Fast Mode (Codex):** every model reports a `priority` tier named "Fast"; a non-empty
  `service_tiers` means the model supports it. `-c service_tier="priority"` is accepted by
  `codex exec --strict-config` and a made-up key is rejected, but the value is not validated and
  the events do not show which tier served a request, so **the effect is unobserved**.
- **Fast Mode in the app:** a role's `fast` flag adds exactly one of the two arguments above, and
  only when it is on; with it off the argument lists are unchanged. The app never records Fast Mode
  use. For a listed Codex model without a Fast tier the app refuses the run; Claude and unreadable
  lists are never refused (support is unknown).
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
- **User config:** Job runs always pass `--ignore-user-config` and `--ignore-rules`.
  Codex still uses its existing login. RemNote uses an explicit server URL, keychain token and
  tool allowlist. The retired “use my Codex config” setting loads as disabled, with no URL or
  approved tools carried into the replacement mode.
- **Shell without attachments:** `--sandbox read-only --disable shell_tool --disable unified_exec`.
  Attachments (PDF, Markdown and text) are staged as files in `anexos/`. The app uses Codex's shell
  to read them. Attachment runs use `approval_policy="never"` and limit the shell subprocess's
  environment inheritance to core variables.
- **macOS and Linux attachment profile:** The app selects a unique permissions profile via
  `-c default_permissions="<name>"`, without `--sandbox`. The inline
  `permissions.<name>.filesystem` table grants read access to `:minimal`, the absolute run
  directory and the resolved Codex runtime. The profile disables shell network access.
  macOS also denies both physical Seatbelt scratch directories (`/private/tmp` and
  `/private/var/tmp`). The app resolves standalone executables and Homebrew symlinks and finds
  the native binary behind an npm launcher. Packaged binaries may need their own runtime folder
  for bundled tools and helpers. A loose binary gets a read grant for that file only.
  Neither the user's home nor `CODEX_HOME` is granted. An unresolvable installation or failed
  profile run stops with an error.
- **Platform defaults on codex-cli 0.172.0:** Linux Bubblewrap's `:minimal` provides the system
  runtime (including `/usr`, `/etc`, `/proc` and `/dev`). Directory traversal needed to reach
  the granted paths can expose ancestor metadata. On macOS, `:minimal` gives Seatbelt access to
  parts of the system libraries and framework directories, standard tools under `/bin` and
  `/usr/bin`, and platform configuration and device handles. It can also read applications
  under `/Applications`. The pinned
  [Linux runtime paths](https://github.com/openai/codex/blob/rust-v0.172.0/codex-rs/linux-sandbox/src/bwrap.rs)
  and [Seatbelt rules](https://github.com/openai/codex/blob/rust-v0.172.0/codex-rs/sandboxing/src/seatbelt_read_only_platform_defaults.sbpl)
  describe the platform runtime access in full.
- **Verification:** codex-cli 0.172.0 read a staged attachment and denied an outside canary,
  writing in the run folder and a loopback connection using the Linux CLI under WSL2 and on a
  hosted Apple Silicon macOS 15 runner. The macOS profile also denied reads and writes in both scratch
  directories. Linux scratch writes stayed in the private mount and left no file on the host.
  A separately invoked `exec --strict-config` accepted the settings and rejected an unknown
  config key. These probes used synthetic files and no model calls.
  [Hosted verification](https://github.com/tiagoguerreirodev/Gabaritei/actions/runs/45533386319).
- **Windows attachment fallback (owner decision, #81):** Use the existing `--sandbox read-only`.
  It permits reads outside the run folder. The narrower profile was refused on Windows 11:
  the unelevated backend required the elevated backend on a native Windows 11 Pro host and
  on the hosted Windows 11 ARM64 runner. The hosted elevated backend required effective
  `:root` read permission. The app does not grant that permission to the named profile or
  change the system sandbox setup. On native Windows 11 Pro, the built-in read-only sandbox
  let the shell read the staged attachment and outside synthetic canary and connect to a
  loopback listener. It blocked a write in the run folder.
- **RemNote token:** passed in the CLI process's environment
  (`GABARITEI_REMNOTE_TOKEN`), never as an argument.

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

- A real `claude` / `codex` run end to end from the installed app (login probe, generation, blind check).

## Manual native app pass (#55)

The 2026-11-24 pass used the Gabaritei 0.3.0 native Linux debug build with the frontend embedded,
Claude Code 2.1.331, Codex CLI 0.172.0 and a temporary app data directory containing synthetic
questions only. Both CLIs used their existing logins.

1. **Generate and dispute.** Codex generated one Certo/Errado question. The initial Claude check
   failed before inference because of the schema dialect declaration (#198). With the compatibility
   build, the check was retried after setting the synthetic draft's key to a known wrong option
   in the temporary database. Claude disagreed, Codex revised the question, and Claude accepted
   the revision on the second check. The saved verdict was `fixed` and marked as objected.
2. **Preview and import.** *Revisar pacote* showed one question and the *corrigida e conferida*
   verdict, with the question content and answer details hidden. *Importar tudo* added the
   question to a generated collection and closed the draft.
3. **Re-check while solving.** A separate synthetic question had a known wrong key and a blank
   explanation. In Study mode the AI actions appeared after confirming the answer. Codex
   proposed the correct option. The stored key and audit history changed on applying the proposal,
   and Resolver re-scored the answer from incorrect to correct.
4. **Explain while solving.** *Explicar* called Codex, displayed an explanation in Resolver and
   persisted it to the question.

The initial schema failure appeared as a generic CLI exit code in the preview. The preview
also used plural labels for counts of one. This live pass covers Linux. RemNote verification
is tracked separately in #54.
