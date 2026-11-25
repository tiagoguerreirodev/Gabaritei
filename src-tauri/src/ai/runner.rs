//! Runs one AI job through a local CLI (`claude -p` or `codex exec`).
//!
//! Every run gets a fresh temporary working folder (attachments copied in, the JSON schema
//! written to a file). The CLI is spawned without a shell, with the prompt on stdin, in its own
//! process group so cancel and timeout can stop the whole tree. The final JSON is returned for
//! the caller to validate against its own typed schema. Progress events carry fixed pt-BR
//! labels only, never model text, so a generation can't leak a question through them.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use super::agent::{Agent, Effort};
use crate::error::AppError;

mod process;
mod workspace;

pub use process::run_job;
pub(super) use process::{Parser, Plan};
pub(super) use workspace::Workdir;

/// How a RemNote MCP server is made available to a run.
#[derive(Clone, PartialEq, Eq)]
pub struct RemNoteAccess {
    pub url: String,
    pub token: String,
    /// Exact read-only tools approved for this server. An empty list grants no tools.
    pub tools: Vec<String>,
}

impl std::fmt::Debug for RemNoteAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemNoteAccess")
            .field("url", &self.url)
            .field("tools", &self.tools)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// Everything needed to run one job.
#[derive(Clone)]
pub struct JobSpec {
    pub agent: Agent,
    /// Path to the executable (from discovery or the user's override).
    pub cli: PathBuf,
    pub model: String,
    pub effort: Effort,
    /// Run in the agent's Fast Mode.
    pub fast: bool,
    /// Role instructions (pt-BR): who the agent is and the rules it follows.
    pub instructions: String,
    /// The task, including any data. May contain answer keys: never logged.
    pub prompt: String,
    /// JSON Schema of the final answer.
    pub schema: Value,
    /// Files copied into the run folder for the agent to read.
    pub attachments: Vec<PathBuf>,
    /// Allow web search and fetch.
    pub web: bool,
    pub remnote: Option<RemNoteAccess>,
    pub timeout: Duration,
    /// How long a cancelled or timed-out CLI gets to stop on SIGINT before it is killed.
    pub grace: Duration,
}

impl JobSpec {
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20 * 60);
    pub const DEFAULT_GRACE: Duration = Duration::from_secs(3);

    pub fn new(agent: Agent, cli: PathBuf, model: String, effort: Effort, schema: Value) -> Self {
        Self {
            agent,
            cli,
            model,
            effort,
            fast: false,
            instructions: String::new(),
            prompt: String::new(),
            schema,
            attachments: Vec::new(),
            web: false,
            remnote: None,
            timeout: Self::DEFAULT_TIMEOUT,
            grace: Self::DEFAULT_GRACE,
        }
    }
}

impl std::fmt::Debug for JobSpec {
    /// Never prints the prompt or instructions: they can carry answer keys.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobSpec")
            .field("agent", &self.agent)
            .field("model", &self.model)
            .field("effort", &self.effort)
            .field("prompt_chars", &self.prompt.chars().count())
            .field("attachments", &self.attachments.len())
            .field("web", &self.web)
            .field("remnote", &self.remnote)
            .finish_non_exhaustive()
    }
}

/// The pipeline stage a run is in. `Preparing` until the first stage starts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum GenerationStage {
    #[default]
    Preparing,
    Generating,
    Checking,
    Resolving,
}

/// A stage of a run, for the progress indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ProgressKind {
    Started,
    Thinking,
    Reading,
    Searching,
    Writing,
}

/// A progress event. The label is fixed pt-BR text, never model output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Progress {
    pub kind: ProgressKind,
    pub label: String,
    /// Set only by the job code at the start of a pipeline stage. The job manager keeps the
    /// latest one, so runner events (which carry none) never reset the stage.
    #[serde(skip)]
    #[ts(skip)]
    pub stage: Option<GenerationStage>,
}

impl Progress {
    pub fn new(kind: ProgressKind, label: &str) -> Self {
        Self {
            kind,
            label: label.to_owned(),
            stage: None,
        }
    }

    /// A progress event that starts a pipeline stage.
    pub fn starting(stage: GenerationStage, kind: ProgressKind, label: &str) -> Self {
        Self {
            stage: Some(stage),
            ..Self::new(kind, label)
        }
    }
}

/// What a successful run produced.
#[derive(Debug, Clone, PartialEq)]
pub struct JobOutput {
    /// The final JSON the agent returned.
    pub value: Value,
    /// Cost in US dollars, if the CLI reports it (Claude does; Codex doesn't).
    pub cost_usd: Option<f64>,
    pub duration_ms: u64,
}

impl JobOutput {
    /// Deserializes the value into the caller's typed schema.
    pub fn parse<T: serde::de::DeserializeOwned>(&self) -> Result<T, RunError> {
        serde_json::from_value(self.value.clone())
            .map_err(|error| RunError::InvalidOutput(error.to_string()))
    }
}

/// Why a run didn't produce an answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunError {
    #[error("Não foi possível iniciar o programa de IA. Verifique o caminho em Configurações.")]
    Spawn(String),
    #[error("O programa de IA não está com login ativo. Entre pelo terminal e tente de novo.")]
    NotLoggedIn,
    #[error("A execução foi cancelada.")]
    Cancelled,
    #[error("A execução demorou demais e foi interrompida.")]
    TimedOut,
    #[error("A execução da IA falhou (código {code}).{detail}")]
    Failed { code: String, detail: String },
    #[error("A IA respondeu em um formato inválido. Tente de novo.")]
    InvalidOutput(String),
    #[error("Não foi possível preparar os arquivos da execução: {0}")]
    Workspace(String),
    #[error(
        "O Codex não consegue ler anexos com isolamento nesta instalação. Use o Claude Code ou remova os anexos."
    )]
    CodexAttachmentSandboxUnavailable,
}

impl From<RunError> for AppError {
    /// The `Display` text is pt-BR and omits technical detail, so it can be shown as is.
    fn from(error: RunError) -> Self {
        Self::Invalid(error.to_string())
    }
}

/// Words that mark a tool as one that changes data. Matched against whole words of the name
/// (`create_note`, `createNote`), so `dataset_search` is not caught by `set`.
const WRITE_WORDS: &[&str] = &[
    "create", "update", "delete", "edit", "write", "move", "remove", "append", "replace", "insert",
    "add", "set", "rename", "patch", "put", "post", "upsert", "merge", "clear", "destroy",
    "archive", "modify", "save", "import", "send",
];

/// Whether a tool name looks like it changes data. Defence in depth: agents only get exact,
/// user-approved read-only tools, and anything that looks like a write is refused anyway.
pub fn is_write_like(tool: &str) -> bool {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    for ch in tool.chars() {
        if !ch.is_alphanumeric() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        } else {
            if ch.is_uppercase() && current.chars().last().is_some_and(char::is_lowercase) {
                words.push(std::mem::take(&mut current));
            }
            current.extend(ch.to_lowercase());
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
        .iter()
        .any(|word| WRITE_WORDS.contains(&word.as_str()))
}

/// Whether a tool name is plain: letters, digits, `_` and `-`, 1 to 100 characters. Anything
/// else (a comma, parenthesis, `*` or `:`) could change what an allowlist entry means to a CLI.
pub fn is_valid_tool_name(tool: &str) -> bool {
    (1..=100).contains(&tool.chars().count())
        && tool
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// The approved tools that are safe to hand to an agent: plain names that don't look like
/// writes. Checked again here, whatever settings allowed, because the names end up inside
/// comma-separated CLI arguments.
pub(super) fn read_only_tools(tools: &[String]) -> Vec<&String> {
    tools
        .iter()
        .filter(|tool| is_valid_tool_name(tool) && !is_write_like(tool))
        .collect()
}

/// Stops a run from outside. Cloning shares the same flag.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
pub(crate) mod tests;
