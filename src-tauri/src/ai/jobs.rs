//! One generation at a time, running in the background, with cancel and a status the
//! frontend can read from any page (spec §3.2 "Run behavior").

use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use ts_rs::TS;

use super::runner::{CancelToken, Progress};
use crate::error::{AppError, AppResult};

/// How a finished generation ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum GenerationOutcome {
    /// A draft is ready for review.
    Draft {
        draft_id: String,
    },
    /// pt-BR reason, safe to show.
    Failed {
        message: String,
    },
    Cancelled,
}

/// What the frontend shows about the generation, on any page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GenerationStatus {
    pub running: bool,
    pub started_at: Option<String>,
    /// The latest progress label while running.
    pub progress: Option<Progress>,
    /// How the last generation ended, until dismissed or replaced.
    pub outcome: Option<GenerationOutcome>,
}

#[derive(Default)]
struct Inner {
    running: Option<Running>,
    outcome: Option<GenerationOutcome>,
}

struct Running {
    cancel: CancelToken,
    started_at: String,
    progress: Option<Progress>,
}

/// Runs generations on a background thread, one at a time.
#[derive(Clone, Default)]
pub struct GenerationManager {
    inner: Arc<Mutex<Inner>>,
}

fn lock(inner: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    inner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl GenerationManager {
    /// Starts `work` in the background. `work` returns the new draft's ID. Fails if a
    /// generation is already running. `on_event` sees every progress event and the final
    /// status, for pushing them to the window.
    pub fn start<W, E>(&self, work: W, on_event: E) -> AppResult<()>
    where
        W: FnOnce(&CancelToken, &mut dyn FnMut(Progress)) -> AppResult<String> + Send + 'static,
        E: Fn(&GenerationStatus) + Send + 'static,
    {
        let cancel = CancelToken::new();
        {
            let mut inner = lock(&self.inner);
            if inner.running.is_some() {
                return Err(AppError::Invalid(
                    "Já existe uma geração em andamento. Espere terminar ou cancele.".into(),
                ));
            }
            inner.outcome = None;
            inner.running = Some(Running {
                cancel: cancel.clone(),
                started_at: crate::clock::now(),
                progress: None,
            });
        }
        let manager = self.clone();
        std::thread::spawn(move || {
            let result = work(&cancel, &mut |progress| {
                if let Some(running) = lock(&manager.inner).running.as_mut() {
                    running.progress = Some(progress);
                }
                on_event(&manager.status());
            });
            let outcome = match result {
                _ if cancel.is_cancelled() => GenerationOutcome::Cancelled,
                Ok(draft_id) => GenerationOutcome::Draft { draft_id },
                Err(error) => GenerationOutcome::Failed {
                    message: error.to_string(),
                },
            };
            {
                let mut inner = lock(&manager.inner);
                inner.running = None;
                inner.outcome = Some(outcome);
            }
            on_event(&manager.status());
        });
        Ok(())
    }

    /// Asks the running generation to stop. Returns whether one was running.
    pub fn cancel(&self) -> bool {
        lock(&self.inner)
            .running
            .as_ref()
            .map(|running| running.cancel.cancel())
            .is_some()
    }

    pub fn status(&self) -> GenerationStatus {
        let inner = lock(&self.inner);
        GenerationStatus {
            running: inner.running.is_some(),
            started_at: inner.running.as_ref().map(|r| r.started_at.clone()),
            progress: inner.running.as_ref().and_then(|r| r.progress.clone()),
            outcome: inner.outcome.clone(),
        }
    }

    /// Forgets the last outcome (the user has seen it).
    pub fn dismiss(&self) {
        lock(&self.inner).outcome = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::runner::ProgressKind;
    use std::sync::mpsc;
    use std::time::Duration;

    fn wait_until_done(manager: &GenerationManager) -> GenerationStatus {
        for _ in 0..200 {
            let status = manager.status();
            if !status.running && status.outcome.is_some() {
                return status;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the generation never finished");
    }

    #[test]
    fn a_successful_run_reports_progress_and_the_draft() {
        let manager = GenerationManager::default();
        let (events_tx, events_rx) = mpsc::channel();
        manager
            .start(
                |_, progress| {
                    progress(Progress::new(ProgressKind::Thinking, "Raciocinando…"));
                    Ok("draft-1".into())
                },
                move |status| {
                    let _ = events_tx.send(status.clone());
                },
            )
            .unwrap();
        let status = wait_until_done(&manager);
        assert_eq!(
            status.outcome,
            Some(GenerationOutcome::Draft {
                draft_id: "draft-1".into()
            })
        );
        let events: Vec<_> = events_rx.try_iter().collect();
        assert!(events.iter().any(|e| {
            e.progress
                .as_ref()
                .is_some_and(|p| p.label == "Raciocinando…")
        }));
        assert!(!events.last().unwrap().running);
    }

    #[test]
    fn only_one_generation_runs_at_a_time() {
        let manager = GenerationManager::default();
        let (release, wait) = mpsc::channel::<()>();
        manager
            .start(
                move |_, _| {
                    let _ = wait.recv();
                    Ok("a".into())
                },
                |_| {},
            )
            .unwrap();
        assert!(manager.status().running);
        let error = manager.start(|_, _| Ok("b".into()), |_| {}).unwrap_err();
        assert!(error.to_string().contains("em andamento"));
        release.send(()).unwrap();
        wait_until_done(&manager);
        // Free again once it finished.
        manager.start(|_, _| Ok("c".into()), |_| {}).unwrap();
        wait_until_done(&manager);
    }

    #[test]
    fn cancel_ends_as_cancelled_even_if_the_work_reports_an_error() {
        let manager = GenerationManager::default();
        manager
            .start(
                |cancel, _| {
                    while !cancel.is_cancelled() {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(AppError::Invalid("A execução foi cancelada.".into()))
                },
                |_| {},
            )
            .unwrap();
        assert!(manager.cancel());
        assert_eq!(
            wait_until_done(&manager).outcome,
            Some(GenerationOutcome::Cancelled)
        );
        assert!(!manager.cancel(), "nothing left to cancel");
    }

    #[test]
    fn failures_carry_the_pt_br_message_and_can_be_dismissed() {
        let manager = GenerationManager::default();
        manager
            .start(
                |_, _| Err(AppError::Invalid("Algo deu errado.".into())),
                |_| {},
            )
            .unwrap();
        let status = wait_until_done(&manager);
        assert_eq!(
            status.outcome,
            Some(GenerationOutcome::Failed {
                message: "Algo deu errado.".into()
            })
        );
        manager.dismiss();
        assert_eq!(manager.status().outcome, None);
    }
}
