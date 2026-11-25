//! AI features the user asks for on one question while solving (re-check, explain): one at a
//! time, cancellable, and with the re-check proposal kept here until the user confirms it, so a
//! confirmation can never carry a key the backend didn't get from the AI.

use crate::sync::lock;
use std::collections::HashMap;
use std::sync::Mutex;

use schemars::JsonSchema;
use serde::de::DeserializeOwned;

use super::invalid;
use super::recheck::Proposal;
use super::roles::{Role, RoleConfig};
use super::runner::CancelToken;
use super::secrets::Secrets;
use super::settings::{self, ResolvedClis, Sources};
use super::task::{Task, ask, schema_of};
use crate::attempts;
use crate::bank::question::{MAX_REF, MAX_REFS, Question};
use crate::bank::store::load_question;
use crate::db::Database;
use crate::error::AppResult;
use crate::ids::{AttemptId, QuestionId};

#[derive(Default)]
pub struct OnDemand {
    current: Mutex<Option<CancelToken>>,
    proposals: Mutex<HashMap<QuestionId, Proposal>>,
}

impl OnDemand {
    /// Runs `work` unless another on-demand job is running.
    pub fn run<T>(&self, work: impl FnOnce(&CancelToken) -> AppResult<T>) -> AppResult<T> {
        let token = CancelToken::new();
        {
            let mut current = lock(&self.current);
            if current.is_some() {
                return Err(invalid(
                    "Já há uma consulta à IA em andamento. Espere ou cancele.",
                ));
            }
            *current = Some(token.clone());
        }
        // Frees the slot even if `work` panics.
        struct Release<'a>(&'a Mutex<Option<CancelToken>>);
        impl Drop for Release<'_> {
            fn drop(&mut self) {
                *lock(self.0) = None;
            }
        }
        let _release = Release(&self.current);
        work(&token)
    }

    /// Whether a job is running now.
    pub fn is_running(&self) -> bool {
        lock(&self.current).is_some()
    }

    /// Forgets every re-check proposal, because the data they were computed from was replaced.
    pub fn clear_proposals(&self) {
        lock(&self.proposals).clear();
    }

    /// Stops the running job. Returns whether there was one.
    pub fn cancel(&self) -> bool {
        lock(&self.current)
            .as_ref()
            .map(CancelToken::cancel)
            .is_some()
    }

    pub(super) fn keep(&self, question_id: QuestionId, proposal: Proposal) {
        lock(&self.proposals).insert(question_id, proposal);
    }

    /// Takes the proposal for a question, if the AI produced one that is still waiting.
    pub(super) fn take(&self, question_id: &QuestionId) -> Option<Proposal> {
        lock(&self.proposals).remove(question_id)
    }
}

pub(super) struct RevealedTask<'a, Prompt> {
    pub attempt_id: &'a AttemptId,
    pub position: u32,
    pub role: Role,
    pub instructions: String,
    pub prompt: Prompt,
}

pub(super) struct RevealedAnswer<T> {
    pub question: Question,
    pub role: RoleConfig,
    pub answer: T,
}

/// Loads the question only after the attempt permits its reveal, then runs the configured role.
pub(super) fn ask_about_revealed<T: DeserializeOwned + JsonSchema>(
    db: &Database,
    secrets: &Secrets,
    clis: &ResolvedClis,
    task: RevealedTask<'_, impl FnOnce(&Question) -> AppResult<String>>,
    cancel: &CancelToken,
) -> AppResult<RevealedAnswer<T>> {
    let (settings, question) = db.with(|conn| {
        let id = attempts::revealed_question(conn, task.attempt_id, task.position)?;
        let question =
            load_question(conn, &id)?.ok_or_else(|| invalid("Questão não encontrada."))?;
        Ok((settings::load(conn, secrets)?, question))
    })?;
    let role = settings.roles.get(task.role).clone();
    let (answer, _) = ask(
        &settings,
        secrets,
        clis,
        &role,
        Task {
            schema: schema_of::<T>(),
            sources: Sources {
                remnote: false,
                web: true,
            },
            instructions: task.instructions,
            prompt: (task.prompt)(&question)?,
            attachments: Vec::new(),
        },
        cancel,
        &mut |_| {},
    )?;
    Ok(RevealedAnswer {
        question,
        role,
        answer,
    })
}

pub(super) enum SourceError {
    TooMany,
    InvalidReference,
}

pub(super) fn validate_sources(sources: Vec<String>) -> Result<Vec<String>, SourceError> {
    if sources.len() > MAX_REFS {
        return Err(SourceError::TooMany);
    }
    sources
        .into_iter()
        .map(|reference| {
            let trimmed = reference.trim();
            if trimmed.is_empty() || reference.chars().count() > MAX_REF {
                return Err(SourceError::InvalidReference);
            }
            Ok(trimmed.to_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panicking_job_frees_the_slot() {
        let jobs = OnDemand::default();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = jobs.run::<()>(|_| panic!("boom"));
        }));
        assert!(result.is_err());
        jobs.run(|_| Ok(())).unwrap();
    }

    #[test]
    fn only_one_job_runs_at_a_time() {
        let jobs = OnDemand::default();
        let outcome = jobs.run(|_| {
            let error = jobs.run(|_| Ok(())).unwrap_err();
            assert!(error.to_string().contains("em andamento"));
            Ok(1)
        });
        assert_eq!(outcome.unwrap(), 1);
        jobs.run(|_| Ok(())).unwrap();
    }

    #[test]
    fn cancel_reaches_the_running_job() {
        let jobs = OnDemand::default();
        jobs.run(|token| {
            assert!(jobs.cancel());
            assert!(token.is_cancelled());
            Ok(())
        })
        .unwrap();
        assert!(!jobs.cancel(), "the slot is free again");
    }
}
