//! AI jobs run through the local `claude` and `codex` CLIs: runner, prompts, schemas and roles.
//!
//! Never calls provider APIs or touches CLI credentials (AGENTS.md rule 2).
//! The runner (#14) is here; roles, generation and the other jobs build on it (#15–#20).
//! Verified CLI flags are recorded in `docs/ai-cli.md`.

pub mod agent;
mod claude;
mod codex;
pub mod discovery;
pub mod roles;
pub mod runner;
pub mod secrets;
pub mod settings;

pub use agent::{Agent, Effort};
pub use runner::{
    CancelToken, JobOutput, JobSpec, Progress, ProgressKind, RemNoteAccess, RunError,
    is_valid_tool_name, is_write_like, run_job,
};
