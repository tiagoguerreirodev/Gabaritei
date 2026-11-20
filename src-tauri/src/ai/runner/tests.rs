//! Runner tests use fake CLI executables (shell scripts) that print canned output: no real AI
//! calls. The fake scripts need `sh`, so these tests run on Unix only; argument building is
//! covered on every platform.

#![cfg_attr(not(unix), allow(dead_code, unused_imports))] // fake CLIs need `sh`

use super::*;
use serde_json::json;

fn schema() -> Value {
    json!({ "type": "object", "required": ["answer"], "properties": { "answer": { "type": "string" } } })
}

fn spec(agent: Agent, cli: PathBuf) -> JobSpec {
    let mut spec = JobSpec::new(agent, cli, "modelo-x".into(), Effort::Xhigh, schema());
    spec.prompt = "pergunta".into();
    spec.instructions = "instruções".into();
    spec.timeout = Duration::from_secs(20);
    spec.grace = Duration::from_millis(300);
    spec
}

fn run(spec: &JobSpec) -> (Result<JobOutput, RunError>, Vec<Progress>) {
    let mut events = Vec::new();
    let result = run_job(spec, &CancelToken::new(), &mut |p| events.push(p));
    (result, events)
}

#[cfg(unix)]
pub(crate) fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A fake `codex exec`: finds `-o`, reads the prompt from stdin, writes the answer there.
#[cfg(unix)]
fn fake_codex(dir: &Path, answer_json: &str) -> PathBuf {
    write_script(
        dir,
        "codex",
        &format!(
            r#"out=""
while [ $# -gt 0 ]; do case "$1" in -o) out="$2"; shift;; esac; shift; done
cat > /dev/null
echo '{{"type":"turn.started"}}'
echo '{{"type":"item.completed","item":{{"type":"reasoning","text":"SEGREDO-DO-MODELO"}}}}'
echo '{{"type":"item.completed","item":{{"type":"agent_message","text":"SEGREDO-DO-MODELO"}}}}'
cat > "$out" <<'JSON'
{answer_json}
JSON
echo '{{"type":"turn.completed","usage":{{}}}}'"#
        ),
    )
}

#[cfg(unix)]
fn pid_is_alive(pid: u32) -> bool {
    let output = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let stat = String::from_utf8_lossy(&output.stdout);
    let stat = stat.trim();
    !stat.is_empty() && !stat.starts_with('Z')
}

#[cfg(unix)]
mod fake_cli {
    use super::*;

    #[test]
    fn codex_success_returns_the_json_and_label_only_progress() {
        let dir = tempfile::tempdir().unwrap();
        let cli = fake_codex(dir.path(), r#"{"answer":"pong"}"#);
        let (result, events) = run(&spec(Agent::Codex, cli));
        let output = result.unwrap();
        assert_eq!(output.value, json!({ "answer": "pong" }));
        assert_eq!(output.cost_usd, None);
        assert_eq!(events.first().unwrap().kind, ProgressKind::Started);
        assert_eq!(events.last().unwrap().kind, ProgressKind::Writing);
        // Model text never reaches the frontend through progress events.
        assert!(events.iter().all(|e| !e.label.contains("SEGREDO")));
    }

    #[test]
    fn the_prompt_goes_on_stdin_not_in_the_arguments() {
        let dir = tempfile::tempdir().unwrap();
        // Echo the arguments and stdin back as the answer.
        let cli = write_script(
            dir.path(),
            "codex",
            r#"out=""; all="$*"
while [ $# -gt 0 ]; do case "$1" in -o) out="$2"; shift;; esac; shift; done
input=$(cat | tr '\n' ' ')
printf '{"args":"%s","stdin":"%s"}' "$all" "$input" > "$out""#,
        );
        let mut job = spec(Agent::Codex, cli);
        job.prompt = "GABARITO-SECRETO".into();
        let value = run(&job).0.unwrap().value;
        assert!(!value["args"].as_str().unwrap().contains("GABARITO-SECRETO"));
        let stdin = value["stdin"].as_str().unwrap();
        assert!(stdin.contains("GABARITO-SECRETO") && stdin.contains("instruções"));
    }

    #[test]
    fn invalid_json_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let cli = fake_codex(dir.path(), "isto não é json");
        assert!(matches!(
            run(&spec(Agent::Codex, cli)).0,
            Err(RunError::InvalidOutput(_))
        ));
    }

    #[test]
    fn output_that_does_not_fit_the_typed_schema_is_rejected() {
        #[derive(Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        #[allow(dead_code)]
        struct Answer {
            answer: String,
        }
        let dir = tempfile::tempdir().unwrap();
        let cli = fake_codex(dir.path(), r#"{"answer":"a","extra":1}"#);
        let output = run(&spec(Agent::Codex, cli)).0.unwrap();
        assert!(matches!(
            output.parse::<Answer>(),
            Err(RunError::InvalidOutput(_))
        ));
    }

    #[test]
    fn a_non_zero_exit_reports_the_code_and_the_last_stderr_line() {
        let dir = tempfile::tempdir().unwrap();
        let cli = write_script(
            dir.path(),
            "codex",
            "cat >/dev/null\necho 'ENUNCIADO-SECRETO e gabarito B' >&2\nexit 3",
        );
        match run(&spec(Agent::Codex, cli)).0 {
            Err(error @ RunError::Failed { .. }) => {
                let RunError::Failed { code, detail } = &error else {
                    unreachable!()
                };
                assert_eq!(code, "3");
                // The CLI's own text never reaches the message: it can hold question content.
                assert_eq!(detail, "");
                assert!(!error.to_string().contains("SECRETO"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn a_usage_limit_gets_a_fixed_hint_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let cli = write_script(
            dir.path(),
            "claude",
            r#"cat >/dev/null
echo '{"type":"result","is_error":true,"result":"Usage limit reached. ENUNCIADO-SECRETO"}'
exit 1"#,
        );
        match run(&spec(Agent::Claude, cli)).0 {
            Err(error) => {
                let message = error.to_string();
                assert!(message.contains("limite de uso"), "{message}");
                assert!(!message.contains("SECRETO"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn a_login_failure_is_reported_as_such() {
        let dir = tempfile::tempdir().unwrap();
        let cli = write_script(
            dir.path(),
            "codex",
            "cat >/dev/null\necho '{\"type\":\"error\",\"message\":\"Not logged in. Run codex login\"}'\nexit 1",
        );
        assert_eq!(run(&spec(Agent::Codex, cli)).0, Err(RunError::NotLoggedIn));
    }

    #[test]
    fn a_missing_executable_is_a_spawn_error() {
        let result = run(&spec(Agent::Codex, PathBuf::from("/no/such/codex"))).0;
        assert!(matches!(result, Err(RunError::Spawn(_))));
    }

    #[test]
    fn timeout_stops_the_run_and_its_children() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("child.pid");
        let cli = write_script(
            dir.path(),
            "codex",
            &format!(
                "sleep 60 &\necho $! > '{}'\ncat >/dev/null\nsleep 60",
                pidfile.display()
            ),
        );
        let mut job = spec(Agent::Codex, cli);
        job.timeout = Duration::from_millis(400);
        let started = Instant::now();
        assert_eq!(run(&job).0, Err(RunError::TimedOut));
        assert!(started.elapsed() < Duration::from_secs(10));
        let pid: u32 = std::fs::read_to_string(&pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert!(!pid_is_alive(pid), "the child process outlived the run");
    }

    #[test]
    fn cancel_stops_the_run_even_when_the_cli_ignores_sigint() {
        let dir = tempfile::tempdir().unwrap();
        let cli = write_script(
            dir.path(),
            "codex",
            "trap '' INT\ncat >/dev/null\nwhile true; do sleep 1; done",
        );
        let job = spec(Agent::Codex, cli);
        let token = CancelToken::new();
        let canceller = token.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            canceller.cancel();
        });
        let started = Instant::now();
        let result = run_job(&job, &token, &mut |_| {});
        assert_eq!(result, Err(RunError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn attachments_and_schema_are_in_a_fresh_folder_that_is_removed_afterwards() {
        let dir = tempfile::tempdir().unwrap();
        let attachment = dir.path().join("Lei 8.112.md");
        std::fs::write(&attachment, "texto").unwrap();
        let cli = write_script(
            dir.path(),
            "codex",
            r#"out=""
while [ $# -gt 0 ]; do case "$1" in -o) out="$2"; shift;; esac; shift; done
cat >/dev/null
printf '{"cwd":"%s","files":"%s"}' "$(pwd)" "$(ls . anexos | tr '\n' ' ')" > "$out""#,
        );
        let mut job = spec(Agent::Codex, cli);
        job.attachments = vec![attachment.clone()];
        let value = run(&job).0.unwrap().value;
        let files = value["files"].as_str().unwrap();
        assert!(
            files.contains("schema.json") && files.contains("1-Lei_8.112.md"),
            "{files}"
        );
        let cwd = Path::new(value["cwd"].as_str().unwrap());
        assert!(!cwd.exists(), "the run folder must be deleted");
        assert!(attachment.exists(), "the original attachment is untouched");
    }

    #[test]
    fn a_missing_attachment_fails_before_any_spawn() {
        let dir = tempfile::tempdir().unwrap();
        let cli = fake_codex(dir.path(), "{}");
        let mut job = spec(Agent::Codex, cli);
        job.attachments = vec![dir.path().join("sumiu.pdf")];
        assert!(matches!(run(&job).0, Err(RunError::Workspace(_))));
    }

    fn fake_claude(dir: &Path, result_line: &str) -> PathBuf {
        write_script(
            dir,
            "claude",
            &format!(
                r#"cat >/dev/null
echo '{{"type":"system","subtype":"init","tools":["Read"]}}'
echo '{{"type":"assistant","message":{{"content":[{{"type":"tool_use","name":"Read","input":{{}}}}]}}}}'
echo '{{"type":"assistant","message":{{"content":[{{"type":"text","text":"SEGREDO-DO-MODELO"}}]}}}}'
cat <<'JSON'
{result_line}
JSON"#
            ),
        )
    }

    #[test]
    fn claude_success_reads_structured_output_and_cost() {
        let dir = tempfile::tempdir().unwrap();
        let cli = fake_claude(
            dir.path(),
            r#"{"type":"result","subtype":"success","is_error":false,"structured_output":{"answer":"pong"},"total_cost_usd":0.0273}"#,
        );
        let (result, events) = run(&spec(Agent::Claude, cli));
        let output = result.unwrap();
        assert_eq!(output.value, json!({ "answer": "pong" }));
        assert_eq!(output.cost_usd, Some(0.0273));
        let kinds: Vec<_> = events.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, [ProgressKind::Started, ProgressKind::Reading]);
    }

    #[test]
    fn claude_without_structured_output_is_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let cli = fake_claude(
            dir.path(),
            r#"{"type":"result","subtype":"success","is_error":false,"result":"texto solto"}"#,
        );
        assert!(matches!(
            run(&spec(Agent::Claude, cli)).0,
            Err(RunError::InvalidOutput(_))
        ));
    }

    #[test]
    fn claude_error_result_with_a_login_message_is_not_logged_in() {
        let dir = tempfile::tempdir().unwrap();
        let cli = write_script(
            dir.path(),
            "claude",
            r#"cat >/dev/null
echo '{"type":"result","subtype":"success","is_error":true,"result":"Not logged in · Please run /login"}'
exit 1"#,
        );
        assert_eq!(run(&spec(Agent::Claude, cli)).0, Err(RunError::NotLoggedIn));
    }
}

mod arguments {
    use super::*;

    fn plan_for(spec: &JobSpec) -> (Vec<String>, Vec<(String, String)>) {
        let work = Workdir::prepare(spec).unwrap();
        let plan = match spec.agent {
            Agent::Claude => claude::plan(spec, &work),
            Agent::Codex => codex::plan(spec, &work),
        };
        (
            plan.args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect(),
            plan.env,
        )
    }

    fn has_pair(args: &[String], flag: &str, value: &str) -> bool {
        args.windows(2).any(|w| w[0] == flag && w[1] == value)
    }

    #[test]
    fn claude_is_read_only_isolated_and_never_bare() {
        let (args, env) = plan_for(&spec(Agent::Claude, "claude".into()));
        assert!(has_pair(&args, "--tools", "Read,Grep,Glob"));
        // File tools are scoped to the run folder.
        assert!(has_pair(
            &args,
            "--allowedTools",
            "Read(./**),Grep(./**),Glob(./**)"
        ));
        assert!(has_pair(&args, "--permission-mode", "dontAsk"));
        assert!(has_pair(&args, "--setting-sources", ""));
        assert!(args.contains(&"--model=modelo-x".to_owned()));
        assert!(has_pair(&args, "--effort", "xhigh"));
        assert!(has_pair(&args, "--output-format", "stream-json"));
        assert!(args.contains(&"--strict-mcp-config".to_owned()));
        assert!(args.contains(&"--no-session-persistence".to_owned()));
        assert!(
            !args.contains(&"--bare".to_owned()),
            "bare mode skips the login"
        );
        for forbidden in ["Bash", "Edit", "Write", "NotebookEdit"] {
            assert!(!args.iter().any(|a| a.contains(forbidden)), "{forbidden}");
        }
        assert!(env.is_empty());
    }

    #[test]
    fn claude_gets_web_tools_only_when_asked() {
        let mut job = spec(Agent::Claude, "claude".into());
        job.web = true;
        let (args, _) = plan_for(&job);
        assert!(has_pair(
            &args,
            "--tools",
            "Read,Grep,Glob,WebSearch,WebFetch"
        ));
    }

    #[test]
    fn claude_remnote_token_lives_only_in_the_temporary_config_file() {
        let mut job = spec(Agent::Claude, "claude".into());
        job.remnote = Some(RemNoteAccess::Server {
            url: "https://remnote.example/mcp".into(),
            token: "TOKEN-SECRETO".into(),
            tools: vec!["search".into(), "read_note".into()],
        });
        let work = Workdir::prepare(&job).unwrap();
        let plan = claude::plan(&job, &work);
        let args: Vec<String> = plan
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(!args.iter().any(|a| a.contains("TOKEN-SECRETO")));
        assert!(plan.env.iter().all(|(_, v)| v != "TOKEN-SECRETO"));
        let config = work.mcp_config.as_ref().unwrap();
        let file = std::fs::read_to_string(config).unwrap();
        assert!(file.contains("TOKEN-SECRETO"));
        // Outside the working directory, so the agent's file tools (scoped to it) can't read it.
        assert!(!config.starts_with(work.path()));
        // Only the exact tools the user approved, never the whole server.
        assert!(has_pair(
            &args,
            "--allowedTools",
            "Read(./**),Grep(./**),Glob(./**),mcp__remnote__search,mcp__remnote__read_note"
        ));
        assert!(!args.iter().any(|a| a.contains("--disallowedTools")));
    }

    #[test]
    fn codex_is_read_only_ignores_user_config_and_closes_the_shell_without_attachments() {
        let (args, _) = plan_for(&spec(Agent::Codex, "codex".into()));
        assert!(has_pair(&args, "--sandbox", "read-only"));
        assert!(args.contains(&"--model=modelo-x".to_owned()));
        assert!(has_pair(&args, "-c", "model_reasoning_effort=xhigh"));
        assert!(has_pair(&args, "-c", "web_search=disabled"));
        assert!(has_pair(&args, "--disable", "shell_tool"));
        assert!(args.contains(&"--ignore-user-config".to_owned()));
        assert!(args.contains(&"--strict-config".to_owned()));
        assert!(args.contains(&"--ephemeral".to_owned()));
        assert!(!args.iter().any(|a| a.contains("dangerously")));
        assert_eq!(args[..2], ["exec".to_owned(), "-".to_owned()]);
    }

    #[test]
    fn codex_keeps_its_read_only_shell_when_there_are_attachments() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "x").unwrap();
        let mut job = spec(Agent::Codex, "codex".into());
        job.attachments = vec![file];
        job.web = true;
        let (args, _) = plan_for(&job);
        assert!(!args.contains(&"shell_tool".to_owned()));
        assert!(has_pair(&args, "--sandbox", "read-only"));
        // Web search is independent of the attachments (the owner decided so).
        assert!(has_pair(&args, "-c", "web_search=live"));
    }

    #[test]
    fn codex_gets_web_search_only_when_asked() {
        let mut job = spec(Agent::Codex, "codex".into());
        job.web = true;
        let (args, _) = plan_for(&job);
        assert!(has_pair(&args, "-c", "web_search=live"));
        assert!(has_pair(&args, "--disable", "shell_tool"));
    }

    #[test]
    fn remnote_with_no_approved_tools_gives_codex_no_server() {
        let mut job = spec(Agent::Codex, "codex".into());
        job.remnote = Some(RemNoteAccess::Server {
            url: "https://remnote.example/mcp".into(),
            token: "t".into(),
            tools: Vec::new(),
        });
        let (args, env) = plan_for(&job);
        assert!(!args.iter().any(|a| a.contains("mcp_servers")));
        assert!(env.is_empty());
    }

    #[test]
    fn codex_remnote_token_travels_in_the_environment_only() {
        let mut job = spec(Agent::Codex, "codex".into());
        job.remnote = Some(RemNoteAccess::Server {
            url: "https://remnote.example/mcp".into(),
            token: "TOKEN-SECRETO".into(),
            tools: vec!["search".into()],
        });
        let (args, env) = plan_for(&job);
        assert!(!args.iter().any(|a| a.contains("TOKEN-SECRETO")));
        assert_eq!(
            env,
            [(codex::TOKEN_ENV.to_owned(), "TOKEN-SECRETO".to_owned())]
        );
        assert!(
            args.iter()
                .any(|a| a.starts_with("mcp_servers.remnote.url="))
        );
    }

    #[test]
    fn reusing_the_codex_config_keeps_the_users_config_loaded() {
        let mut job = spec(Agent::Codex, "codex".into());
        job.remnote = Some(RemNoteAccess::CodexConfig);
        let (args, env) = plan_for(&job);
        assert!(!args.contains(&"--ignore-user-config".to_owned()));
        assert!(env.is_empty());
    }

    #[test]
    fn debug_output_hides_the_prompt_and_the_token() {
        let mut job = spec(Agent::Claude, "claude".into());
        job.prompt = "GABARITO-SECRETO".into();
        job.remnote = Some(RemNoteAccess::Server {
            url: "https://x".into(),
            token: "TOKEN-SECRETO".into(),
            tools: Vec::new(),
        });
        let shown = format!("{job:?}");
        assert!(!shown.contains("GABARITO-SECRETO") && !shown.contains("TOKEN-SECRETO"));
    }

    #[test]
    fn write_like_tool_names_are_recognised_by_whole_words() {
        for name in [
            "create_note",
            "createNote",
            "update-rem",
            "delete",
            "set_title",
            "AddTag",
        ] {
            assert!(is_write_like(name), "{name}");
        }
        for name in [
            "search",
            "read_note",
            "get_rem",
            "dataset_search",
            "list_tags",
            "offset",
        ] {
            assert!(!is_write_like(name), "{name}");
        }
    }

    #[test]
    fn tool_names_that_could_change_an_allowlist_entry_are_dropped() {
        for bad in [
            "search,Bash",
            "Bash(*)",
            "a*",
            "x:y",
            "",
            "has space",
            "../x",
        ] {
            assert!(!is_valid_tool_name(bad), "{bad:?}");
        }
        assert!(is_valid_tool_name("read_note") && is_valid_tool_name("get-rem"));

        let mut claude_job = spec(Agent::Claude, "claude".into());
        claude_job.remnote = Some(RemNoteAccess::Server {
            url: "https://remnote.example/mcp".into(),
            token: "t".into(),
            tools: vec!["search,Bash".into(), "ok_tool".into()],
        });
        let (args, _) = plan_for(&claude_job);
        let allowed = args
            .windows(2)
            .find(|w| w[0] == "--allowedTools")
            .map(|w| w[1].clone())
            .unwrap();
        assert!(!allowed.contains("Bash"));
        assert!(allowed.ends_with("mcp__remnote__ok_tool"));
    }

    #[test]
    fn write_like_tools_are_dropped_from_the_plan_for_both_agents() {
        let remnote = Some(RemNoteAccess::Server {
            url: "https://remnote.example/mcp".into(),
            token: "t".into(),
            tools: vec!["search".into(), "create_note".into()],
        });
        let mut claude_job = spec(Agent::Claude, "claude".into());
        claude_job.remnote = remnote.clone();
        let (args, _) = plan_for(&claude_job);
        assert!(args.iter().any(|a| a.ends_with("mcp__remnote__search")));
        assert!(!args.iter().any(|a| a.contains("create_note")));

        let mut codex_job = spec(Agent::Codex, "codex".into());
        codex_job.remnote = remnote;
        let (args, _) = plan_for(&codex_job);
        assert!(has_pair(
            &args,
            "-c",
            r#"mcp_servers.remnote.enabled_tools=["search"]"#
        ));
    }

    #[test]
    fn a_blind_check_run_puts_no_question_text_in_the_claude_arguments() {
        use crate::ai::prompts::{CheckerView, blind_check_instructions, blind_check_prompt};
        let view = CheckerView {
            id: "q1".into(),
            kind: crate::bank::QuestionType::Boolean,
            stem: "ENUNCIADO-SECRETO".into(),
            options: vec![("c".into(), "Certo".into()), ("e".into(), "Errado".into())],
        };
        let mut job = spec(Agent::Claude, "claude".into());
        job.instructions = blind_check_instructions();
        job.prompt = blind_check_prompt(&[view]);
        let (args, env) = plan_for(&job);
        // The questions travel on stdin only; arguments and environment never carry them.
        assert!(!args.iter().any(|a| a.contains("ENUNCIADO-SECRETO")));
        assert!(env.is_empty());
        assert!(job.prompt.contains("ENUNCIADO-SECRETO"));
        assert!(job.attachments.is_empty());
    }

    #[test]
    fn long_multi_line_instructions_travel_by_file_never_as_an_argument() {
        // Rust refuses newlines in `.cmd` arguments and `cmd.exe` caps the command line at
        // 8191 characters (issue #49), so the role instructions must not be an argument.
        let mut job = spec(Agent::Claude, "claude".into());
        job.instructions = format!("linha um\nlinha dois\r\n{}", "x".repeat(20_000));
        let work = Workdir::prepare(&job).unwrap();
        let plan = claude::plan(&job, &work);
        let args: Vec<String> = plan
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let file = work.instructions.as_ref().expect("instructions file");
        assert!(has_pair(
            &args,
            "--append-system-prompt-file",
            &file.to_string_lossy()
        ));
        assert_eq!(std::fs::read_to_string(file).unwrap(), job.instructions);
        // Outside the run folder, so the agent's file tools can't list or cite it.
        assert!(!file.starts_with(work.path()));
        assert!(!args.iter().any(|a| a.contains("linha um")));
        assert!(!args.iter().any(|a| a.contains(['\n', '\r'])));
        assert!(args.iter().map(String::len).sum::<usize>() < 4_000);
    }

    #[test]
    fn attachment_names_are_made_safe() {
        assert_eq!(sanitize("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitize(".bashrc"), "bashrc");
        assert_eq!(sanitize("Lei 8.112 (1990).pdf"), "Lei_8.112__1990_.pdf");
    }
}

/// The same behaviors on Windows, where npm installs the CLIs as `.cmd` shims (issue #49).
/// The fakes are batch files; `ping -n` is the portable way to sleep in one.
#[cfg(windows)]
mod windows_cmd {
    use super::*;

    fn write_cmd(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(format!("{name}.cmd"));
        std::fs::write(
            &path,
            format!("@echo off\r\n{}\r\n", body.replace('\n', "\r\n")),
        )
        .unwrap();
        path
    }

    /// Counts running `ping.exe` processes whose command line holds `marker`.
    fn processes_with(marker: &str) -> usize {
        let script = format!(
            "(Get-CimInstance Win32_Process -Filter \"Name='ping.exe'\" | \
             Where-Object {{ $_.CommandLine -like '*{marker}*' }} | Measure-Object).Count"
        );
        let output = Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap()
    }

    /// Polls (a slow runner may need a moment) until no `ping.exe` matches any marker.
    fn gone_within_5s(markers: &[&str]) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if markers.iter().all(|m| processes_with(m) == 0) {
                return true;
            }
            if Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    #[test]
    fn a_claude_cmd_shim_accepts_the_schema_and_multi_line_instructions() {
        let dir = tempfile::tempdir().unwrap();
        let cli = write_cmd(
            dir.path(),
            "claude",
            // Fails unless the schema arrives intact and the instructions file exists.
            r#"more >nul
set "schema="
set "file="
:next
if "%~1"=="" goto done
if "%~1"=="--json-schema" set "schema=%~2"
if "%~1"=="--append-system-prompt-file" set "file=%~2"
shift
goto next
:done
if not exist "%file%" exit /b 7
if not "%schema:~0,1%"=="{" exit /b 8
echo {"type":"result","subtype":"success","is_error":false,"structured_output":{"answer":"pong"}}"#,
        );
        let mut job = spec(Agent::Claude, cli);
        job.instructions = "linha um\nlinha dois\n".repeat(2_000);
        let output = run(&job).0.unwrap();
        assert_eq!(output.value, json!({ "answer": "pong" }));
    }

    #[test]
    fn a_codex_cmd_shim_runs_and_writes_its_answer_file() {
        let dir = tempfile::tempdir().unwrap();
        let cli = write_cmd(
            dir.path(),
            "codex",
            r#"set "out="
:next
if "%~1"=="" goto done
if "%~1"=="-o" set "out=%~2"
shift
goto next
:done
more >nul
echo {"type":"turn.started"}
echo {"answer":"pong"}> "%out%"
echo {"type":"turn.completed","usage":{}}"#,
        );
        let (result, events) = run(&spec(Agent::Codex, cli));
        assert_eq!(result.unwrap().value, json!({ "answer": "pong" }));
        assert_eq!(events.first().unwrap().kind, ProgressKind::Started);
    }

    #[test]
    fn timeout_ends_the_whole_process_tree() {
        let dir = tempfile::tempdir().unwrap();
        let cli = write_cmd(
            dir.path(),
            "codex",
            "start /b ping -n 4242 127.0.0.1 >nul\nmore >nul\nping -n 4243 127.0.0.1 >nul",
        );
        let mut job = spec(Agent::Codex, cli);
        job.timeout = Duration::from_secs(2);
        let started = Instant::now();
        assert_eq!(run(&job).0, Err(RunError::TimedOut));
        assert!(started.elapsed() < Duration::from_secs(15));
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(
            processes_with(marker),
            0,
            "a child process outlived the run"
        );
    }

    #[test]
    fn cancel_ends_the_run_quickly() {
        let dir = tempfile::tempdir().unwrap();
        let cli = write_cmd(
            dir.path(),
            "codex",
            "more >nul\nping -n 4244 127.0.0.1 >nul",
        );
        let job = spec(Agent::Codex, cli);
        let token = CancelToken::new();
        let canceller = token.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            canceller.cancel();
        });
        let started = Instant::now();
        assert_eq!(run_job(&job, &token, &mut |_| {}), Err(RunError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(15));
        assert!(gone_within_5s(&["-n 4244"]), "the process outlived the run");
    }
}
