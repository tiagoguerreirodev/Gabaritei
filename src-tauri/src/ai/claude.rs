//! Claude Code (`claude -p`): arguments and output parsing. Flags are verified in
//! `docs/ai-cli.md`.

use serde_json::{Value, json};

use super::runner::{JobSpec, Parser, Plan, Progress, ProgressKind, RunError, Workdir};

/// Built-in tools the agent may use: read-only, no shell, no edits.
const READ_TOOLS: &[&str] = &["Read", "Grep", "Glob"];
const WEB_TOOLS: &[&str] = &["WebSearch", "WebFetch"];
/// Name the RemNote server gets in the MCP config.
const REMNOTE_SERVER: &str = "remnote";
/// Tool-name fragments of RemNote write operations, denied even if the server offers them.
const REMNOTE_WRITE_PATTERNS: &[&str] = &[
    "create", "update", "delete", "edit", "write", "move", "remove", "append", "replace", "insert",
];

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
    let mut allowed: Vec<String> = READ_TOOLS.iter().map(|t| (*t).to_owned()).collect();
    if spec.web {
        allowed.extend(WEB_TOOLS.iter().map(|t| (*t).to_owned()));
    }
    // `--tools` limits the built-ins that exist at all; MCP tools are allowed separately.
    let builtin = allowed.join(",");
    let mut denied: Vec<String> = Vec::new();
    if work.mcp_config.is_some() {
        allowed.push(format!("mcp__{REMNOTE_SERVER}"));
        denied.extend(
            REMNOTE_WRITE_PATTERNS
                .iter()
                .map(|p| format!("mcp__{REMNOTE_SERVER}__*{p}*")),
        );
    }

    let mut args: Vec<String> = vec![
        "-p".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--json-schema".into(),
        spec.schema.to_string(),
        "--model".into(),
        spec.model.clone(),
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
    if !denied.is_empty() {
        args.push("--disallowedTools".into());
        args.push(denied.join(","));
    }
    if let Some(config) = &work.mcp_config {
        args.push("--mcp-config".into());
        args.push(config.to_string_lossy().into_owned());
    }
    if !spec.instructions.is_empty() {
        args.push("--append-system-prompt".into());
        args.push(spec.instructions.clone());
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
