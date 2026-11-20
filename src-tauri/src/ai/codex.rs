//! Codex (`codex exec`): arguments and output parsing. Flags are verified in `docs/ai-cli.md`.

use serde_json::Value;

use super::runner::{
    JobSpec, Parser, Plan, Progress, ProgressKind, RemNoteAccess, RunError, Workdir,
};

/// Environment variable that carries the RemNote token, so it never appears in arguments.
pub(super) const TOKEN_ENV: &str = "GABARITEI_REMNOTE_TOKEN";

/// Quotes a string as a TOML value for `-c key=value`.
fn toml_string(value: &str) -> String {
    serde_json::to_string(value).expect("a string serializes")
}

pub(super) fn plan(spec: &JobSpec, work: &Workdir) -> Plan {
    // Web search and a file-reading shell are never on together: a prompt-injected attachment
    // could otherwise read local files and send them out through a web request.
    let web = spec.web && !work.has_attachments;
    let mut args: Vec<String> = vec![
        "exec".into(),
        // The prompt is on stdin.
        "-".into(),
        "--output-schema".into(),
        work.schema.to_string_lossy().into_owned(),
        "-o".into(),
        work.output.to_string_lossy().into_owned(),
        "--json".into(),
        "--sandbox".into(),
        "read-only".into(),
        "--skip-git-repo-check".into(),
        "--ephemeral".into(),
        "--ignore-rules".into(),
        "-m".into(),
        spec.model.clone(),
        "-c".into(),
        format!("model_reasoning_effort={}", spec.effort.as_str()),
        // Fail on a config key this version doesn't know, instead of silently ignoring it.
        "--strict-config".into(),
        "-c".into(),
        format!("web_search={}", if web { "live" } else { "disabled" }),
    ];
    // The user's own Codex config (hooks, MCP servers, instructions) is ignored, except when the
    // user chose to reuse it for RemNote.
    if !matches!(spec.remnote, Some(RemNoteAccess::CodexConfig)) {
        args.push("--ignore-user-config".into());
    }
    // Codex reads files through its shell. Without attachments there is nothing to read, so the
    // shell is switched off; with them it stays, inside the read-only sandbox.
    if !work.has_attachments {
        for feature in ["shell_tool", "unified_exec"] {
            args.push("--disable".into());
            args.push(feature.into());
        }
    }
    let mut env = Vec::new();
    if let Some(RemNoteAccess::Server { url, token, tools }) = &spec.remnote
        && !tools.is_empty()
    {
        args.push("-c".into());
        args.push(format!("mcp_servers.remnote.url={}", toml_string(url)));
        args.push("-c".into());
        args.push(format!(
            "mcp_servers.remnote.bearer_token_env_var={}",
            toml_string(TOKEN_ENV)
        ));
        args.push("-c".into());
        args.push(format!(
            "mcp_servers.remnote.enabled_tools={}",
            serde_json::to_string(tools).expect("a list of strings serializes")
        ));
        env.push((TOKEN_ENV.to_owned(), token.clone()));
    }
    Plan {
        args: args.into_iter().map(Into::into).collect(),
        env,
    }
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
