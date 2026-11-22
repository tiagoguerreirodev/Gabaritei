//! Claude Code (`claude -p`): arguments and output parsing. Flags are verified in
//! `docs/ai-cli.md`.

use serde_json::{Value, json};

use super::runner::{
    JobSpec, Parser, Plan, Progress, ProgressKind, RemNoteAccess, RunError, Workdir,
    read_only_tools,
};

/// Built-in tools the agent may use: read-only, no shell, no edits.
const READ_TOOLS: &[&str] = &["Read", "Grep", "Glob"];
const WEB_TOOLS: &[&str] = &["WebSearch", "WebFetch"];
/// Name the RemNote server gets in the MCP config.
const REMNOTE_SERVER: &str = "remnote";

/// The MCP config file for the RemNote server.
pub(super) fn mcp_config(url: &str, token: &str) -> String {
    json!({
        "mcpServers": {
            REMNOTE_SERVER: {
                "type": "http",
                "url": url,
                "headers": { "Authorization": format!("Bearer {token}") }
            }
        }
    })
    .to_string()
}

pub(super) fn plan(spec: &JobSpec, work: &Workdir) -> Plan {
    // `--tools` limits which built-ins exist at all.
    let mut builtin: Vec<String> = READ_TOOLS.iter().map(|t| (*t).to_owned()).collect();
    if spec.web {
        builtin.extend(WEB_TOOLS.iter().map(|t| (*t).to_owned()));
    }
    // `--allowedTools` is what dontAsk mode lets run. File tools are scoped to the run folder
    // (the working directory), so the agent can't read the user's other files.
    let mut allowed: Vec<String> = READ_TOOLS.iter().map(|t| format!("{t}(./**)")).collect();
    if spec.web {
        allowed.extend(WEB_TOOLS.iter().map(|t| (*t).to_owned()));
    }
    // RemNote: only the exact read-only tools the user approved, never the whole server.
    if work.mcp_config.is_some()
        && let Some(RemNoteAccess::Server { tools, .. }) = &spec.remnote
    {
        allowed.extend(
            read_only_tools(tools)
                .into_iter()
                .map(|tool| format!("mcp__{REMNOTE_SERVER}__{tool}")),
        );
    }
    let builtin = builtin.join(",");

    let mut args: Vec<String> = vec![
        "-p".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--json-schema".into(),
        spec.schema.to_string(),
        format!("--model={}", spec.model),
        "--effort".into(),
        spec.effort.as_str().into(),
        // Load no user, project or local settings: no hooks, plugins or CLAUDE.md of the user.
        "--setting-sources".into(),
        String::new(),
        "--strict-mcp-config".into(),
        "--tools".into(),
        builtin,
        "--allowedTools".into(),
        allowed.join(","),
        "--permission-mode".into(),
        "dontAsk".into(),
        "--no-session-persistence".into(),
        "--disable-slash-commands".into(),
    ];
    if spec.fast {
        // Fast Mode is an opt-in setting in print mode (verified in docs/ai-cli.md).
        args.push("--settings".into());
        args.push(r#"{"fastMode":true}"#.into());
    }
    if let Some(config) = &work.mcp_config {
        args.push("--mcp-config".into());
        args.push(config.to_string_lossy().into_owned());
    }
    if let Some(file) = &work.instructions {
        args.push("--append-system-prompt-file".into());
        args.push(file.to_string_lossy().into_owned());
    }
    // `RemNoteAccess::CodexConfig` only makes sense for Codex; Claude then gets no RemNote.
    Plan {
        args: args.into_iter().map(Into::into).collect(),
        env: Vec::new(),
    }
}

/// Reads Claude's `stream-json` output: the last `result` event holds the answer.
#[derive(Default)]
pub(super) struct ClaudeParser {
    result: Option<Value>,
}

impl Parser for ClaudeParser {
    fn feed(&mut self, line: &str) -> Option<Progress> {
        let event: Value = serde_json::from_str(line).ok()?;
        match event.get("type")?.as_str()? {
            "system" if event.get("subtype").and_then(Value::as_str) == Some("init") => {
                Some(Progress::new(ProgressKind::Started, "Iniciando…"))
            }
            "assistant" => event["message"]["content"]
                .as_array()?
                .iter()
                .find_map(tool_progress),
            "result" => {
                self.result = Some(event);
                None
            }
            _ => None,
        }
    }

    fn finish(&mut self, _work: &Workdir) -> Result<(Value, Option<f64>), RunError> {
        let result = self
            .result
            .take()
            .ok_or_else(|| RunError::InvalidOutput("no result event".into()))?;
        let cost = result.get("total_cost_usd").and_then(Value::as_f64);
        match result.get("structured_output") {
            Some(value) if !value.is_null() => Ok((value.clone(), cost)),
            _ => Err(RunError::InvalidOutput("no structured_output".into())),
        }
    }

    fn error_text(&self) -> Option<String> {
        let result = self.result.as_ref()?;
        if result.get("is_error").and_then(Value::as_bool) == Some(true) {
            result
                .get("result")
                .and_then(Value::as_str)
                .map(str::to_owned)
        } else {
            None
        }
    }
}

fn tool_progress(block: &Value) -> Option<Progress> {
    if block.get("type")?.as_str()? != "tool_use" {
        return None;
    }
    let name = block.get("name")?.as_str()?;
    Some(match name {
        "Read" | "Grep" | "Glob" => Progress::new(ProgressKind::Reading, "Lendo as fontes…"),
        "WebSearch" | "WebFetch" => Progress::new(ProgressKind::Searching, "Pesquisando na web…"),
        "StructuredOutput" => Progress::new(ProgressKind::Writing, "Redigindo a resposta…"),
        n if n.starts_with("mcp__") => {
            Progress::new(ProgressKind::Searching, "Consultando o RemNote…")
        }
        _ => Progress::new(ProgressKind::Thinking, "Raciocinando…"),
    })
}
