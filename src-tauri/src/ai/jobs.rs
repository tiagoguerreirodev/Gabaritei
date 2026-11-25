//! One generation at a time, running in the background, with cancel and a status the
//! frontend can read from any page (spec §3.2 "Run behavior").

use crate::sync::lock;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use ts_rs::TS;

pub use super::runner::GenerationStage;
use super::runner::{CancelToken, Progress};
use crate::error::{AppError, AppResult};
use crate::ids::DraftId;

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
        draft_id: DraftId,
    },
    /// pt-BR reason, safe to show.
    Failed {
        message: String,
    },
    Cancelled,
}

/// Which kind of run holds the background slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum GenerationKind {
    /// Generate, blind-check and resolve disputes.
    Generate,
    /// Only the blind check (and dispute round) of a saved draft.
    Recheck,
}

/// What the frontend shows about the generation, on any page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GenerationStatus {
    pub running: bool,
    pub started_at: Option<String>,
    /// What kind of run is going, while running.
    pub kind: Option<GenerationKind>,
    /// The stage of the running job (`Preparing` when not running).
    pub stage: GenerationStage,
    /// The latest progress label while running.
    pub progress: Option<Progress>,
    /// How the last generation ended, until dismissed or replaced.
    pub outcome: Option<GenerationOutcome>,
    /// The kind of run that produced `outcome`, so its wording can tell a re-check from a
    /// generation.
    pub outcome_kind: Option<GenerationKind>,
}

#[derive(Default)]
struct Inner {
    running: Option<Running>,
    outcome: Option<GenerationOutcome>,
    outcome_kind: Option<GenerationKind>,
}

struct Running {
    cancel: CancelToken,
    started_at: String,
    kind: GenerationKind,
    stage: GenerationStage,
    progress: Option<Progress>,
}

/// Runs generations on a background thread, one at a time.
#[derive(Clone, Default)]
pub struct GenerationManager {
    inner: Arc<Mutex<Inner>>,
}

impl GenerationManager {
    /// Starts `work` in the background. `work` returns the new draft's ID. Fails if a
    /// generation is already running. `on_event` sees every progress event and the final
    /// status, for pushing them to the window.
    pub fn start<W, E>(
        &self,
        kind: GenerationKind,
        started_at: String,
        work: W,
        on_event: E,
    ) -> AppResult<()>
    where
        W: FnOnce(&CancelToken, &mut dyn FnMut(Progress)) -> AppResult<DraftId> + Send + 'static,
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
            inner.outcome_kind = None;
            inner.running = Some(Running {
                cancel: cancel.clone(),
                started_at,
                kind,
                stage: GenerationStage::Preparing,
                progress: None,
            });
        }
        let manager = self.clone();
        std::thread::spawn(move || {
            // A panic must not leave the slot held forever.
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                work(&cancel, &mut |progress| {
                    if let Some(running) = lock(&manager.inner).running.as_mut() {
                        if let Some(stage) = progress.stage {
                            // Stages only move forward, whatever order events arrive in.
                            running.stage = running.stage.max(stage);
                        }
                        running.progress = Some(progress);
                    }
                    on_event(&manager.status());
                })
            }))
            .unwrap_or_else(|_| {
                Err(AppError::Invalid(
                    "A geração parou por um erro inesperado. Tente de novo.".into(),
                ))
            });
            // A draft that was saved wins over a cancel that arrived afterwards.
            let outcome = match result {
                Ok(draft_id) => GenerationOutcome::Draft { draft_id },
                Err(_) if cancel.is_cancelled() => GenerationOutcome::Cancelled,
                Err(error) => GenerationOutcome::Failed {
                    message: error.to_string(),
                },
            };
            {
                let mut inner = lock(&manager.inner);
                inner.outcome_kind = inner.running.as_ref().map(|r| r.kind);
                inner.running = None;
                inner.outcome = Some(outcome);
            }
            let _ = std::panic::catch_unwind(AssertUnwindSafe(|| on_event(&manager.status())));
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
            kind: inner.running.as_ref().map(|r| r.kind),
            stage: inner
                .running
                .as_ref()
                .map_or_else(Default::default, |r| r.stage),
            progress: inner.running.as_ref().and_then(|r| r.progress.clone()),
            outcome: inner.outcome.clone(),
            outcome_kind: inner.outcome_kind,
        }
    }

    /// Forgets the last outcome (the user has seen it).
    pub fn dismiss(&self) {
        let mut inner = lock(&self.inner);
        inner.outcome = None;
        inner.outcome_kind = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::runner::ProgressKind;
    use std::sync::mpsc;
    use std::time::Duration;

    const STARTED_AT: &str = "2026-11-20T12:00:00.000Z";

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

    fn successful_generation() -> (GenerationStatus, Vec<GenerationStatus>) {
        let manager = GenerationManager::default();
        let (events_tx, events_rx) = mpsc::channel();
        manager
            .start(
                GenerationKind::Generate,
                STARTED_AT.into(),
                |_, progress| {
                    progress(Progress::starting(
                        GenerationStage::Checking,
                        ProgressKind::Thinking,
                        "Raciocinando…",
                    ));
                    progress(Progress::new(ProgressKind::Reading, "Lendo…"));
                    Ok("draft-1".into())
                },
                move |status| {
                    let _ = events_tx.send(status.clone());
                },
            )
            .unwrap();
        let done = wait_until_done(&manager);
        let events = (0..3)
            .map(|_| events_rx.recv_timeout(Duration::from_secs(5)).unwrap())
            .collect();
        (done, events)
    }

    #[test]
    fn a_successful_generation_reports_its_draft() {
        let (done, _) = successful_generation();
        assert_eq!(
            done.outcome,
            Some(GenerationOutcome::Draft {
                draft_id: "draft-1".into()
            }),
        );
    }

    #[test]
    fn running_generation_broadcasts_progress() {
        let (_, events) = successful_generation();
        assert_eq!(
            events[0].progress,
            Some(Progress::starting(
                GenerationStage::Checking,
                ProgressKind::Thinking,
                "Raciocinando…",
            )),
        );
        assert_eq!(
            events[1].progress,
            Some(Progress::new(ProgressKind::Reading, "Lendo…")),
        );
        assert!(!events[2].running);
    }

    #[test]
    fn runner_progress_without_a_stage_preserves_the_current_stage() {
        let (_, events) = successful_generation();
        assert_eq!(events[1].stage, GenerationStage::Checking);
        assert_eq!(events[1].kind, Some(GenerationKind::Generate));
        assert_eq!(events[2].stage, GenerationStage::Preparing);
    }

    #[test]
    fn running_generation_reports_the_supplied_start_time() {
        let (_, events) = successful_generation();
        assert_eq!(events[0].started_at.as_deref(), Some(STARTED_AT));
    }

    fn recheck_statuses() -> (GenerationStatus, GenerationStatus) {
        let manager = GenerationManager::default();
        let (release, wait) = mpsc::channel::<()>();
        let (events, started) = mpsc::channel();
        manager
            .start(
                GenerationKind::Recheck,
                STARTED_AT.into(),
                move |_, progress| {
                    progress(Progress::starting(
                        GenerationStage::Checking,
                        ProgressKind::Started,
                        "Conferindo às cegas…",
                    ));
                    let _ = wait.recv();
                    Ok("d".into())
                },
                move |status| {
                    let _ = events.send(status.clone());
                },
            )
            .unwrap();
        let running = started.recv_timeout(Duration::from_secs(5)).unwrap();
        release.send(()).unwrap();
        let done = wait_until_done(&manager);
        (running, done)
    }

    #[test]
    fn a_recheck_reports_its_kind() {
        let (running, done) = recheck_statuses();
        assert_eq!(running.kind, Some(GenerationKind::Recheck));
        assert_eq!(done.kind, None);
        assert_eq!(done.outcome_kind, Some(GenerationKind::Recheck));
    }

    #[test]
    fn a_finished_recheck_resets_the_current_stage() {
        let (running, done) = recheck_statuses();
        assert_eq!(running.stage, GenerationStage::Checking);
        assert_eq!(done.stage, GenerationStage::Preparing);
    }

    #[test]
    fn the_outcome_remembers_its_kind_until_dismissed() {
        let manager = GenerationManager::default();
        manager
            .start(
                GenerationKind::Recheck,
                STARTED_AT.into(),
                |cancel, _| {
                    cancel.cancel();
                    Err(AppError::Invalid("x".into()))
                },
                |_| {},
            )
            .unwrap();
        let done = wait_until_done(&manager);
        assert_eq!(done.outcome, Some(GenerationOutcome::Cancelled));
        assert_eq!(done.outcome_kind, Some(GenerationKind::Recheck));
        manager.dismiss();
        assert_eq!(manager.status().outcome_kind, None);
    }

    fn panicking_generation() -> (GenerationManager, GenerationStatus) {
        let manager = GenerationManager::default();
        manager
            .start(
                GenerationKind::Generate,
                STARTED_AT.into(),
                |_, progress| {
                    progress(Progress::starting(
                        GenerationStage::Checking,
                        ProgressKind::Started,
                        "x",
                    ));
                    panic!("segredo do estudo");
                },
                |_| {},
            )
            .unwrap();
        let done = wait_until_done(&manager);
        (manager, done)
    }

    #[test]
    fn a_panicking_run_reports_a_generic_failure() {
        let (_, done) = panicking_generation();
        let Some(GenerationOutcome::Failed { message }) = done.outcome else {
            panic!("expected a failure");
        };
        assert!(!message.contains("segredo"));
    }

    #[test]
    fn a_panicking_run_frees_the_slot() {
        let (manager, done) = panicking_generation();
        assert_eq!(done.kind, None);
        assert_eq!(done.stage, GenerationStage::Preparing);
        manager
            .start(
                GenerationKind::Generate,
                STARTED_AT.into(),
                |_, _| Ok("again".into()),
                |_| {},
            )
            .unwrap();
        wait_until_done(&manager);
    }

    #[test]
    fn a_panicking_event_handler_also_frees_the_slot() {
        let manager = GenerationManager::default();
        manager
            .start(
                GenerationKind::Generate,
                STARTED_AT.into(),
                |_, progress| {
                    progress(Progress::new(ProgressKind::Thinking, "x"));
                    Ok("d".into())
                },
                |status| {
                    if status.running {
                        panic!("handler");
                    }
                },
            )
            .unwrap();
        let done = wait_until_done(&manager);
        assert!(matches!(
            done.outcome,
            Some(GenerationOutcome::Failed { .. })
        ));
        assert!(!done.running);
    }

    #[test]
    fn stages_never_move_backwards() {
        let manager = GenerationManager::default();
        let (release, wait) = mpsc::channel::<()>();
        manager
            .start(
                GenerationKind::Generate,
                STARTED_AT.into(),
                move |_, progress| {
                    progress(Progress::starting(
                        GenerationStage::Resolving,
                        ProgressKind::Started,
                        "a",
                    ));
                    progress(Progress::starting(
                        GenerationStage::Checking,
                        ProgressKind::Started,
                        "b",
                    ));
                    let _ = wait.recv();
                    Ok("d".into())
                },
                |_| {},
            )
            .unwrap();
        for _ in 0..200 {
            if manager.status().progress.is_some_and(|p| p.label == "b") {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(manager.status().stage, GenerationStage::Resolving);
        release.send(()).unwrap();
        wait_until_done(&manager);
    }

    #[test]
    fn only_one_generation_runs_at_a_time() {
        let manager = GenerationManager::default();
        let (release, wait) = mpsc::channel::<()>();
        manager
            .start(
                GenerationKind::Generate,
                STARTED_AT.into(),
                move |_, _| {
                    let _ = wait.recv();
                    Ok("a".into())
                },
                |_| {},
            )
            .unwrap();
        assert!(manager.status().running);
        let error = manager
            .start(
                GenerationKind::Generate,
                STARTED_AT.into(),
                |_, _| Ok("b".into()),
                |_| {},
            )
            .unwrap_err();
        assert!(error.to_string().contains("em andamento"));
        release.send(()).unwrap();
        wait_until_done(&manager);
        // Free again once it finished.
        manager
            .start(
                GenerationKind::Generate,
                STARTED_AT.into(),
                |_, _| Ok("c".into()),
                |_| {},
            )
            .unwrap();
        wait_until_done(&manager);
    }

    #[test]
    fn cancel_ends_as_cancelled_even_if_the_work_reports_an_error() {
        let manager = GenerationManager::default();
        manager
            .start(
                GenerationKind::Generate,
                STARTED_AT.into(),
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
    fn a_saved_draft_is_reported_even_if_cancel_arrived_late() {
        let manager = GenerationManager::default();
        manager
            .start(
                GenerationKind::Generate,
                STARTED_AT.into(),
                |cancel, _| {
                    cancel.cancel();
                    Ok("draft-9".into())
                },
                |_| {},
            )
            .unwrap();
        assert_eq!(
            wait_until_done(&manager).outcome,
            Some(GenerationOutcome::Draft {
                draft_id: "draft-9".into()
            })
        );
    }

    fn failed_generation() -> (GenerationManager, GenerationStatus) {
        let manager = GenerationManager::default();
        manager
            .start(
                GenerationKind::Generate,
                STARTED_AT.into(),
                |_, _| Err(AppError::Invalid("Algo deu errado.".into())),
                |_| {},
            )
            .unwrap();
        let status = wait_until_done(&manager);
        (manager, status)
    }

    #[test]
    fn failures_carry_the_pt_br_message() {
        let (_, status) = failed_generation();
        assert_eq!(
            status.outcome,
            Some(GenerationOutcome::Failed {
                message: "Algo deu errado.".into()
            }),
        );
    }

    #[test]
    fn a_failure_can_be_dismissed() {
        let (manager, status) = failed_generation();
        assert!(status.outcome.is_some());
        manager.dismiss();
        assert_eq!(manager.status().outcome, None);
    }
}
