//! Child-process isolation, output streaming, cancellation and exit handling.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{Agent, CancelToken, JobOutput, JobSpec, Progress, RunError, Workdir};
use crate::ai::{claude, codex, discovery};

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

/// Arguments and environment for one spawn.
pub(in crate::ai) struct Plan {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(String, String)>,
}

/// Reads one agent's output stream.
pub(in crate::ai) trait Parser: Send {
    /// Looks at one stdout line; returns a progress event if it marks a stage.
    fn feed(&mut self, line: &str) -> Option<Progress>;
    /// The final JSON and cost, once the process has ended.
    fn finish(&mut self, work: &Workdir) -> Result<(Value, Option<f64>), RunError>;
    /// A short error text the CLI reported, for failure messages.
    fn error_text(&self) -> Option<String>;
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
            codex::plan(spec, work)?,
            Box::new(codex::CodexParser::default()),
        ),
    };
    let mut command = Command::new(&plan.executable);
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
    let child = discovery::spawn_retrying(&mut command)
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
