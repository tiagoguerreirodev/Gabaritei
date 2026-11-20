//! Runs one AI job through a local CLI (`claude -p` or `codex exec`).
//!
//! Every run gets a fresh temporary working folder (attachments copied in, the JSON schema
//! written to a file). The CLI is spawned without a shell, with the prompt on stdin, in its own
//! process group so cancel and timeout can stop the whole tree. The final JSON is returned for
//! the caller to validate against its own typed schema. Progress events carry fixed pt-BR
//! labels only, never model text, so a generation can't leak a question through them.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use super::agent::{Agent, Effort};
use super::{claude, codex};
use crate::error::AppError;

/// Environment variables that would make a CLI bill an API key instead of using the login the
/// user made. The app only ever uses the logged-in CLIs (AGENTS.md rule 2).
const SCRUBBED_ENV: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "OPENAI_API_KEY",
    "CODEX_API_KEY",
];

/// How much of the CLI's stderr we keep for error messages.
const STDERR_KEEP: usize = 8 * 1024;
const POLL: Duration = Duration::from_millis(50);

/// How a RemNote MCP server is made available to a run.
#[derive(Clone, PartialEq, Eq)]
pub enum RemNoteAccess {
    /// The app passes the server's URL and token to the CLI. Only the named tools may be used:
    /// an allowlist of exact read-only tool names, so a write tool can never run. With no
    /// names, the agent gets no RemNote tools at all.
    Server {
        url: String,
        token: String,
        tools: Vec<String>,
    },
    /// Codex only: use the RemNote server already in the user's own Codex config.
    CodexConfig,
}

impl std::fmt::Debug for RemNoteAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Server { url, tools, .. } => f
                .debug_struct("Server")
                .field("url", url)
                .field("tools", tools)
                .field("token", &"<redacted>")
                .finish(),
            Self::CodexConfig => f.write_str("CodexConfig"),
        }
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
    Done,
}

/// A progress event. The label is fixed pt-BR text, never model output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Progress {
    pub kind: ProgressKind,
    pub label: String,
}

impl Progress {
    pub fn new(kind: ProgressKind, label: &str) -> Self {
        Self {
            kind,
            label: label.to_owned(),
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

/// The run's temporary folder and the files the CLI is pointed at. Deleted on drop.
pub(super) struct Workdir {
    dir: tempfile::TempDir,
    /// Holds `mcp.json`, which contains the RemNote token. It is a separate folder, not under
    /// the agent's working directory, so the agent's file tools can't read the token.
    _config_dir: Option<tempfile::TempDir>,
    pub schema: PathBuf,
    /// Claude's role instructions, passed by file. On Windows an npm install is a `.cmd` shim,
    /// and Rust refuses newlines in the arguments of batch files (and `cmd.exe` caps a command
    /// line at 8191 characters), so long multi-line text must not travel as an argument.
    pub instructions: Option<PathBuf>,
    /// Where Codex writes its last message (`-o`).
    pub output: PathBuf,
    /// Claude's MCP config, if RemNote is on (outside the working directory).
    pub mcp_config: Option<PathBuf>,
    pub has_attachments: bool,
}

impl Workdir {
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    fn prepare(spec: &JobSpec) -> Result<Self, RunError> {
        let workspace = |error: std::io::Error| RunError::Workspace(error.to_string());
        let dir = tempfile::Builder::new()
            .prefix("gabaritei-run-")
            .tempdir()
            .map_err(workspace)?;
        let schema = dir.path().join("schema.json");
        std::fs::write(
            &schema,
            serde_json::to_vec_pretty(&spec.schema).unwrap_or_default(),
        )
        .map_err(workspace)?;

        let instructions = if spec.agent == Agent::Claude && !spec.instructions.is_empty() {
            let path = dir.path().join("instrucoes.md");
            std::fs::write(&path, &spec.instructions).map_err(workspace)?;
            Some(path)
        } else {
            None
        };

        if !spec.attachments.is_empty() {
            let folder = dir.path().join("anexos");
            std::fs::create_dir(&folder).map_err(workspace)?;
            for (index, source) in spec.attachments.iter().enumerate() {
                let name = source
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "arquivo".into());
                let target = folder.join(format!("{}-{}", index + 1, sanitize(&name)));
                std::fs::copy(source, target).map_err(|error| {
                    RunError::Workspace(format!("{}: {error}", sanitize(&name)))
                })?;
            }
        }

        let (config_dir, mcp_config) = match (&spec.agent, &spec.remnote) {
            (Agent::Claude, Some(RemNoteAccess::Server { url, token, .. })) => {
                let folder = tempfile::Builder::new()
                    .prefix("gabaritei-config-")
                    .tempdir()
                    .map_err(workspace)?;
                let path = folder.path().join("mcp.json");
                write_private(&path, &claude::mcp_config(url, token)).map_err(workspace)?;
                (Some(folder), Some(path))
            }
            _ => (None, None),
        };

        Ok(Self {
            _config_dir: config_dir,
            output: dir.path().join("saida.json"),
            schema,
            instructions,
            mcp_config,
            has_attachments: !spec.attachments.is_empty(),
            dir,
        })
    }
}

/// Writes a file only the current user can read (on Unix; Windows folders under the user's
/// temp directory are already private to them).
fn write_private(path: &Path, content: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(content.as_bytes())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, content)
    }
}

/// A file name that is safe inside the run folder on every platform.
fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    cleaned.trim_start_matches('.').to_owned()
}

/// Arguments and environment for one spawn.
pub(super) struct Plan {
    pub args: Vec<OsString>,
    pub env: Vec<(String, String)>,
}

/// Reads one agent's output stream.
pub(super) trait Parser: Send {
    /// Looks at one stdout line; returns a progress event if it marks a stage.
    fn feed(&mut self, line: &str) -> Option<Progress>;
    /// The final JSON and cost, once the process has ended.
    fn finish(&mut self, work: &Workdir) -> Result<(Value, Option<f64>), RunError>;
    /// A short error text the CLI reported, for failure messages.
    fn error_text(&self) -> Option<String>;
}

fn looks_like_login_problem(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "not logged in",
        "please log in",
        "please run /login",
        "login required",
        "codex login",
        "unauthorized",
        "401",
        "authentication",
        "invalid api key",
        "token expired",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn spawn(spec: &JobSpec, work: &Workdir) -> Result<(Child, Box<dyn Parser>), RunError> {
    let (plan, parser): (Plan, Box<dyn Parser>) = match spec.agent {
        Agent::Claude => (
            claude::plan(spec, work),
            Box::new(claude::ClaudeParser::default()),
        ),
        Agent::Codex => (
            codex::plan(spec, work),
            Box::new(codex::CodexParser::default()),
        ),
    };
    let mut command = Command::new(&spec.cli);
    command
        .args(&plan.args)
        .current_dir(work.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in SCRUBBED_ENV {
        command.env_remove(key);
    }
    for (key, value) in &plan.env {
        command.env(key, value);
    }
    isolate(&mut command);
    let child = command
        .spawn()
        .map_err(|error| RunError::Spawn(error.to_string()))?;
    Ok((child, parser))
}

/// Puts the CLI in its own process group (Unix) or without a console window (Windows), so the
/// whole tree can be stopped and no console flashes on screen.
fn isolate(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

/// Stops the process and everything it started: SIGINT, then a kill after `grace`.
/// The kill is sent even if the main process exited on SIGINT, to catch stragglers.
fn terminate(child: &mut Child, grace: Duration) {
    #[cfg(unix)]
    {
        let group = -(child.id() as i32);
        // SAFETY: plain signal syscalls on a process group we created.
        unsafe { libc::kill(group, libc::SIGINT) };
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline && matches!(child.try_wait(), Ok(None)) {
            std::thread::sleep(Duration::from_millis(10));
        }
        unsafe { libc::kill(group, libc::SIGKILL) };
    }
    #[cfg(windows)]
    {
        // No SIGINT on Windows: end the tree right away.
        let _ = grace;
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn read_stderr(mut stderr: impl Read) -> String {
    let mut text = String::new();
    let mut buffer = [0u8; 4096];
    while let Ok(read) = stderr.read(&mut buffer) {
        if read == 0 {
            break;
        }
        text.push_str(&String::from_utf8_lossy(&buffer[..read]));
        if text.len() > STDERR_KEEP * 2 {
            let cut = text.len() - STDERR_KEEP;
            let cut = (cut..text.len())
                .find(|i| text.is_char_boundary(*i))
                .unwrap_or(text.len());
            text.drain(..cut);
        }
    }
    text
}

/// A short pt-BR hint for failures we recognize. The CLI's own text is never shown: after a
/// generation it can contain question text or a key, and it would reach the banner.
fn known_failure(text: &str) -> String {
    let lower = text.to_lowercase();
    let hint = if [
        "rate limit",
        "usage limit",
        "quota",
        "too many requests",
        "429",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        " O limite de uso da conta foi atingido; tente mais tarde."
    } else if ["overloaded", "503", "529", "unavailable"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        " O serviço de IA está sobrecarregado; tente mais tarde."
    } else {
        ""
    };
    hint.to_owned()
}

/// Runs the job and waits for it. Blocking: call it from a worker thread.
pub fn run_job(
    spec: &JobSpec,
    cancel: &CancelToken,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<JobOutput, RunError> {
    let started = Instant::now();
    let work = Workdir::prepare(spec)?;
    let (mut child, mut parser) = spawn(spec, &work)?;

    // The prompt goes on stdin: it stays out of the process list and has no length limit.
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let prompt = if spec.agent == Agent::Codex {
        // Codex has no system-prompt flag, so the instructions lead the prompt.
        format!("{}\n\n{}", spec.instructions, spec.prompt)
    } else {
        spec.prompt.clone()
    };
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(prompt.as_bytes());
    });

    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let (tx, rx) = mpsc::channel::<String>();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let stderr_reader = std::thread::spawn(move || read_stderr(stderr));

    let deadline = started + spec.timeout;
    let mut last_progress: Option<Progress> = None;
    let interrupted = |child: &mut Child| -> Option<RunError> {
        if cancel.is_cancelled() {
            terminate(child, spec.grace);
            Some(RunError::Cancelled)
        } else if Instant::now() >= deadline {
            terminate(child, spec.grace);
            Some(RunError::TimedOut)
        } else {
            None
        }
    };

    let mut stopped = None;
    loop {
        if let Some(error) = interrupted(&mut child) {
            stopped = Some(error);
            break;
        }
        match rx.recv_timeout(POLL) {
            Ok(line) => {
                if let Some(progress) = parser.feed(&line)
                    && last_progress.as_ref() != Some(&progress)
                {
                    on_progress(progress.clone());
                    last_progress = Some(progress);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let status: Option<ExitStatus> = if stopped.is_none() {
        loop {
            if let Some(error) = interrupted(&mut child) {
                stopped = Some(error);
                break None;
            }
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => std::thread::sleep(POLL),
                Err(_) => break None,
            }
        }
    } else {
        None
    };
    let _ = writer.join();
    let _ = reader.join();
    let stderr_text = stderr_reader.join().unwrap_or_default();
    if let Some(error) = stopped {
        return Err(error);
    }

    let status = status.ok_or_else(|| RunError::Failed {
        code: "?".into(),
        detail: String::new(),
    })?;
    if !status.success() {
        let reported = parser.error_text().unwrap_or_default();
        if looks_like_login_problem(&reported) || looks_like_login_problem(&stderr_text) {
            return Err(RunError::NotLoggedIn);
        }
        let detail = known_failure(&format!("{reported}\n{stderr_text}"));
        return Err(RunError::Failed {
            code: status
                .code()
                .map_or_else(|| "sinal".to_owned(), |code| code.to_string()),
            detail,
        });
    }
    let (value, cost_usd) = parser.finish(&work)?;
    Ok(JobOutput {
        value,
        cost_usd,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

#[cfg(test)]
pub(crate) mod tests;
