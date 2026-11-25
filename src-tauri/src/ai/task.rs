//! Running one structured job for a role, shared by every AI feature.

use super::roles::RoleConfig;
use super::runner::{CancelToken, JobOutput, Progress, run_job};
use super::secrets::Secrets;
use super::settings::{self, AiSettings, ResolvedClis, Sources};
use crate::error::AppResult;

/// What one structured job asks of a role.
pub(super) struct Task {
    pub schema: serde_json::Value,
    pub sources: Sources,
    pub instructions: String,
    pub prompt: String,
    /// Files copied into the run folder for the agent to read.
    pub attachments: Vec<std::path::PathBuf>,
}

/// Runs one structured job for `config`.
pub(super) fn ask<T: serde::de::DeserializeOwned>(
    settings: &AiSettings,
    secrets: &Secrets,
    clis: &ResolvedClis,
    config: &RoleConfig,
    task: Task,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(Progress),
) -> AppResult<(T, JobOutput)> {
    let spec = settings::job_spec(
        settings,
        secrets,
        clis,
        config,
        task.schema,
        task.sources,
        task.attachments,
    )?
    .with_input(task.instructions, task.prompt);
    let output = run_job(&spec, cancel, progress)?;
    Ok((output.parse()?, output))
}
