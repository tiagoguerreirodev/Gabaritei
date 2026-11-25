//! AI features the user asks for on one question while solving (re-check, explain): one at a
//! time, cancellable, and with the re-check proposal kept here until the user confirms it, so a
//! confirmation can never carry a key the backend didn't get from the AI.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use super::recheck::Proposal;
use super::runner::CancelToken;
use crate::error::{AppError, AppResult};
use crate::ids::QuestionId;

#[derive(Default)]
pub struct OnDemand {
    current: Mutex<Option<CancelToken>>,
    proposals: Mutex<HashMap<QuestionId, Proposal>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl OnDemand {
    /// Runs `work` unless another on-demand job is running.
    pub fn run<T>(&self, work: impl FnOnce(&CancelToken) -> AppResult<T>) -> AppResult<T> {
        let token = CancelToken::new();
        {
            let mut current = lock(&self.current);
            if current.is_some() {
                return Err(AppError::Invalid(
                    "Já há uma consulta à IA em andamento. Espere ou cancele.".into(),
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

    pub fn keep(&self, question_id: QuestionId, proposal: Proposal) {
        lock(&self.proposals).insert(question_id, proposal);
    }

    /// Takes the proposal for a question, if the AI produced one that is still waiting.
    pub fn take(&self, question_id: &QuestionId) -> Option<Proposal> {
        lock(&self.proposals).remove(question_id)
    }
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
