//! Codex (`codex exec`): arguments and output parsing. Flags are verified in `docs/ai-cli.md`.

use std::path::PathBuf;

use serde_json::Value;

use super::REMNOTE_SERVER;
use super::runner::{
    JobSpec, Parser, Plan, Progress, ProgressKind, RemNoteAccess, RunError, Workdir,
    read_only_tools,
};

mod install;

/// Environment variable that carries the RemNote token, so it never appears in arguments.
pub(super) const TOKEN_ENV: &str = "GABARITEI_REMNOTE_TOKEN";

const MACOS_SCRATCH_PATHS: &[&str] = &["/private/tmp", "/private/var/tmp"];

/// Quotes a string as a TOML value for `-c key=value`.
fn toml_string(value: &str) -> String {
    serde_json::to_string(value).expect("a string serializes")
}

pub(super) fn plan(spec: &JobSpec, work: &Workdir) -> Result<Plan, RunError> {
    let mut args: Vec<String> = vec![
        "exec".into(),
        // The prompt is on stdin.
        "-".into(),
        "--output-schema".into(),
        work.schema.to_string_lossy().into_owned(),
        "-o".into(),
        work.output.to_string_lossy().into_owned(),
        "--json".into(),
        "--skip-git-repo-check".into(),
        "--ephemeral".into(),
        "--ignore-rules".into(),
        "--ignore-user-config".into(),
        format!("--model={}", spec.model),
        "-c".into(),
        format!("model_reasoning_effort={}", spec.effort.as_str()),
        // Fail on a config key this version doesn't know, instead of silently ignoring it.
        "--strict-config".into(),
        "-c".into(),
        format!("web_search={}", if spec.web { "live" } else { "disabled" }),
    ];
    if spec.fast {
        args.push("-c".into());
        args.push(r#"service_tier="priority""#.into());
    }
    let executable = if work.has_attachments && !cfg!(windows) {
        restrict_attachment_shell(spec, work, &mut args)?
    } else {
        // Windows rejects a restricted-read profile on both sandbox backends.
        args.extend(["--sandbox".into(), "read-only".into()]);
        if !work.has_attachments {
            for feature in ["shell_tool", "unified_exec"] {
                args.extend(["--disable".into(), feature.into()]);
            }
        }
        spec.cli.clone()
    };
    if work.has_attachments {
        for value in [
            r#"approval_policy="never""#,
            r#"shell_environment_policy.inherit="core""#,
        ] {
            args.extend(["-c".into(), value.into()]);
        }
    }
    let mut env = Vec::new();
    if let Some(RemNoteAccess { url, token, tools }) = &spec.remnote
        && let tools = read_only_tools(tools)
        && !tools.is_empty()
    {
        args.push("-c".into());
        args.push(format!(
            "mcp_servers.{REMNOTE_SERVER}.url={}",
            toml_string(url)
        ));
        args.push("-c".into());
        args.push(format!(
            "mcp_servers.{REMNOTE_SERVER}.bearer_token_env_var={}",
            toml_string(TOKEN_ENV)
        ));
        args.push("-c".into());
        args.push(format!(
            "mcp_servers.{REMNOTE_SERVER}.enabled_tools={}",
            serde_json::to_string(&tools).expect("a list of strings serializes")
        ));
        env.push((TOKEN_ENV.to_owned(), token.clone()));
    }
    Ok(Plan {
        executable,
        args: args.into_iter().map(Into::into).collect(),
        env,
    })
}

pub(super) const fn supports_attachment_profile() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos"))
}

fn restrict_attachment_shell(
    spec: &JobSpec,
    work: &Workdir,
    args: &mut Vec<String>,
) -> Result<PathBuf, RunError> {
    use RunError::CodexAttachmentSandboxUnavailable;
    if !supports_attachment_profile() {
        return Err(CodexAttachmentSandboxUnavailable);
    }
    let installation = install::resolve(&spec.cli).ok_or(CodexAttachmentSandboxUnavailable)?;
    let run_folder = work
        .path()
        .canonicalize()
        .map_err(|_| CodexAttachmentSandboxUnavailable)?;
    let name = run_folder
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(CodexAttachmentSandboxUnavailable)?;
    let mut scratch_denials = String::new();
    if cfg!(target_os = "macos") {
        // Seatbelt adds these directories as writable scratch when `:minimal` is enabled.
        for scratch in MACOS_SCRATCH_PATHS {
            if run_folder.starts_with(scratch) || installation.readable.starts_with(scratch) {
                return Err(CodexAttachmentSandboxUnavailable);
            }
            scratch_denials.push_str(&format!(", {scratch:?}=\"deny\""));
        }
    }
    let run_folder = run_folder
        .to_str()
        .ok_or(CodexAttachmentSandboxUnavailable)?;
    let readable = installation
        .readable
        .to_str()
        .ok_or(CodexAttachmentSandboxUnavailable)?;
    let run_folder = toml_string(run_folder);
    let readable = toml_string(readable);
    let profile = format!(
        "permissions.{name}.filesystem={{\":minimal\"=\"read\", {readable}=\"read\", {run_folder}=\"read\"{scratch_denials}}}"
    );
    for value in [
        format!("default_permissions={}", toml_string(name)),
        profile,
        format!("permissions.{name}.network.enabled=false"),
    ] {
        args.extend(["-c".into(), value]);
    }
    Ok(installation.executable)
}

/// Reads Codex's JSONL events. The answer is the last message, written to the `-o` file.
#[derive(Default)]
pub(super) struct CodexParser {
    last_message: Option<String>,
    error: Option<String>,
}

impl Parser for CodexParser {
    fn feed(&mut self, line: &str) -> Option<Progress> {
        let event: Value = serde_json::from_str(line).ok()?;
        match event.get("type")?.as_str()? {
            "turn.started" => Some(Progress::new(ProgressKind::Started, "Iniciando…")),
            "turn.failed" => {
                self.error = event["error"]["message"].as_str().map(str::to_owned);
                None
            }
            "error" => {
                self.error = event["message"].as_str().map(str::to_owned);
                None
            }
            "item.started" | "item.completed" => {
                let item = event.get("item")?;
                match item.get("type")?.as_str()? {
                    "agent_message" => {
                        if event["type"] == "item.completed" {
                            self.last_message = item["text"].as_str().map(str::to_owned);
                        }
                        Some(Progress::new(
                            ProgressKind::Writing,
                            "Redigindo a resposta…",
                        ))
                    }
                    "reasoning" => Some(Progress::new(ProgressKind::Thinking, "Raciocinando…")),
                    "command_execution" => {
                        Some(Progress::new(ProgressKind::Reading, "Lendo as fontes…"))
                    }
                    "web_search" => Some(Progress::new(
                        ProgressKind::Searching,
                        "Pesquisando na web…",
                    )),
                    "mcp_tool_call" => Some(Progress::new(
                        ProgressKind::Searching,
                        "Consultando o RemNote…",
                    )),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn finish(&mut self, work: &Workdir) -> Result<(Value, Option<f64>), RunError> {
        let text = std::fs::read_to_string(&work.output)
            .ok()
            .filter(|text| !text.trim().is_empty())
            .or_else(|| self.last_message.take())
            .ok_or_else(|| RunError::InvalidOutput("no final message".into()))?;
        let value = serde_json::from_str(text.trim())
            .map_err(|error| RunError::InvalidOutput(error.to_string()))?;
        // Codex doesn't report a cost.
        Ok((value, None))
    }

    fn error_text(&self) -> Option<String> {
        self.error.clone()
    }
}
