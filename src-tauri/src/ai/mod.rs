//! AI jobs run through the local `claude` and `codex` CLIs: runner, prompts, schemas and roles.
//!
//! Never calls provider APIs or touches CLI credentials (AGENTS.md rule 2).
//! The runner (#14) is here; roles, generation and the other jobs build on it (#15–#20).
//! Verified CLI flags are recorded in `docs/ai-cli.md`.

pub(crate) mod agent;
pub(crate) mod blind_check;
pub(crate) mod catalog;
mod claude;
mod codex;
pub(crate) mod discovery;
pub(crate) mod draft;
pub(crate) mod draft_files;
pub(crate) mod explain;
pub(crate) mod generate;
pub(crate) mod jobs;
pub(crate) mod ondemand;
mod prompts;
pub(crate) mod recheck;
pub(crate) mod remnote_probe;
pub(crate) mod review;
pub(crate) mod roles;
pub(crate) mod runner;
pub(crate) mod secrets;
pub(crate) mod settings;
mod task;

pub use agent::{Agent, Effort};
pub use runner::{
    CancelToken, JobOutput, JobSpec, Progress, ProgressKind, RemNoteAccess, RunError,
    is_valid_tool_name, is_write_like, run_job,
};

fn invalid(message: &str) -> crate::error::AppError {
    crate::error::AppError::Invalid(message.to_owned())
}

const REMNOTE_SERVER: &str = "remnote";
