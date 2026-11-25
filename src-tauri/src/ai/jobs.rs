//! One generation at a time, running in the background, with cancel and a status the
//! frontend can read from any page (spec §3.2 "Run behavior").

use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use time::OffsetDateTime;
use ts_rs::TS;

use super::ondemand::OnDemand;
pub use super::runner::GenerationStage;
use super::runner::{CancelToken, Progress};
use crate::backup::BackupService;
use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::ids::DraftId;
use crate::paths::AppPaths;
use crate::sync::lock;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GenerationStep {
    pub id: GenerationStage,
    pub label: &'static str,
}

impl GenerationKind {
    fn stages(self) -> Vec<GenerationStep> {
        let generating = GenerationStep {
            id: GenerationStage::Generating,
            label: "Gerando as questões",
        };
        let checking = GenerationStep {
            id: GenerationStage::Checking,
            label: "Conferindo às cegas",
        };
        let resolving = GenerationStep {
            id: GenerationStage::Resolving,
            label: "Resolvendo as contestações",
        };
        match self {
            Self::Generate => vec![generating, checking, resolving],
            Self::Recheck => vec![checking, resolving],
        }
    }

    fn cancel_discards_draft(self, stage: GenerationStage) -> bool {
        match self {
            Self::Generate => match stage {
                GenerationStage::Preparing | GenerationStage::Generating => false,
                GenerationStage::Checking | GenerationStage::Resolving => true,
            },
            Self::Recheck => false,
        }
    }
}

/// What the frontend shows about the generation, on any page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GenerationStatus {
    /// Orders snapshots from IPC reads and events even when they arrive out of order.
    #[ts(type = "number")]
    pub revision: u64,
    /// Revision of the most recent run start or finish, so readers can refresh draft data.
    #[ts(type = "number")]
    pub run_revision: u64,
    pub running: bool,
    pub started_at: Option<String>,
    /// What kind of run is going, while running.
    pub kind: Option<GenerationKind>,
    /// The stage of the running job (`Preparing` when not running).
    pub stage: GenerationStage,
    pub stages: Vec<GenerationStep>,
    pub cancel_discards_draft: bool,
    pub can_cancel: bool,
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
    revision: u64,
    run_revision: u64,
    running: Option<Running>,
    outcome: Option<GenerationOutcome>,
    outcome_kind: Option<GenerationKind>,
}

struct Running {
    cancel: Option<CancelToken>,
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
        let cancel = self.reserve(kind, started_at)?;
        let manager = self.clone();
        std::thread::spawn(move || {
            let result = manager.perform(&on_event, |progress| work(&cancel, progress));
            // A draft that was saved wins over a cancel that arrived afterwards.
            let outcome = match result {
                Ok(draft_id) => GenerationOutcome::Draft { draft_id },
                Err(_) if cancel.is_cancelled() => GenerationOutcome::Cancelled,
                Err(error) => GenerationOutcome::Failed {
                    message: error.to_string(),
                },
            };
            manager.finish(Some(outcome), on_event);
        });
        Ok(())
    }

    /// Holds the same slot as a generation until both the check and its import finish.
    /// The caller receives the result directly, so there is no background outcome to dismiss.
    pub(crate) fn run_check_then_import<T>(
        &self,
        started_at: String,
        work: impl FnOnce(&ImportCheck<'_>, &mut dyn FnMut(Progress)) -> AppResult<T>,
        on_event: impl Fn(&GenerationStatus),
    ) -> AppResult<T> {
        let cancel = self.reserve(GenerationKind::Recheck, started_at)?;
        let run = ImportCheck {
            manager: self,
            cancel,
        };
        let result = self.perform(&on_event, |progress| work(&run, progress));
        self.finish(None, on_event);
        result
    }

    fn reserve(&self, kind: GenerationKind, started_at: String) -> AppResult<CancelToken> {
        let mut inner = lock(&self.inner);
        if inner.running.is_some() {
            return Err(AppError::Invalid(
                "Já existe uma geração em andamento. Espere terminar ou cancele.".into(),
            ));
        }
        let cancel = CancelToken::new();
        inner.revision += 1;
        inner.run_revision = inner.revision;
        inner.outcome = None;
        inner.outcome_kind = None;
        inner.running = Some(Running {
            cancel: Some(cancel.clone()),
            started_at,
            kind,
            stage: GenerationStage::Preparing,
            progress: None,
        });
        Ok(cancel)
    }

    fn perform<T>(
        &self,
        on_event: &impl Fn(&GenerationStatus),
        work: impl FnOnce(&mut dyn FnMut(Progress)) -> AppResult<T>,
    ) -> AppResult<T> {
        // A panic must not leave the slot held forever.
        std::panic::catch_unwind(AssertUnwindSafe(|| {
            work(&mut |progress| {
                let status = {
                    let mut inner = lock(&self.inner);
                    if let Some(running) = inner.running.as_mut() {
                        if let Some(stage) = progress.stage {
                            // Stages only move forward, whatever order events arrive in.
                            running.stage = running.stage.max(stage);
                        }
                        running.progress = Some(progress);
                        inner.revision += 1;
                    }
                    inner.status()
                };
                on_event(&status);
            })
        }))
        .unwrap_or_else(|_| {
            Err(AppError::Invalid(
                "A geração parou por um erro inesperado. Tente de novo.".into(),
            ))
        })
    }

    fn finish(&self, outcome: Option<GenerationOutcome>, on_event: impl Fn(&GenerationStatus)) {
        let status = {
            let mut inner = lock(&self.inner);
            let kind = inner.running.take().map(|running| running.kind);
            inner.outcome_kind = outcome.as_ref().and(kind);
            inner.outcome = outcome;
            inner.revision += 1;
            inner.run_revision = inner.revision;
            inner.status()
        };
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| on_event(&status)));
    }

    fn is_running(&self) -> bool {
        lock(&self.inner).running.is_some()
    }

    /// Asks the running generation to stop. Returns whether one was running.
    pub fn cancel(&self) -> bool {
        lock(&self.inner)
            .running
            .as_ref()
            .and_then(|running| running.cancel.as_ref())
            .map(CancelToken::cancel)
            .is_some()
    }

    pub fn status(&self) -> GenerationStatus {
        lock(&self.inner).status()
    }

    /// Forgets the last outcome (the user has seen it).
    pub fn dismiss(&self) {
        let mut inner = lock(&self.inner);
        if inner.outcome.take().is_some() {
            inner.outcome_kind = None;
            inner.revision += 1;
        }
    }
}

impl Inner {
    fn status(&self) -> GenerationStatus {
        let running = self.running.as_ref();
        let kind = running.map(|r| r.kind);
        let stage = running.map_or_else(Default::default, |r| r.stage);
        let can_cancel = running.is_some_and(|run| run.cancel.is_some());
        GenerationStatus {
            revision: self.revision,
            run_revision: self.run_revision,
            running: running.is_some(),
            started_at: running.map(|r| r.started_at.clone()),
            kind,
            stage,
            stages: kind.map_or_else(Vec::new, GenerationKind::stages),
            cancel_discards_draft: can_cancel
                && kind.is_some_and(|kind| kind.cancel_discards_draft(stage)),
            can_cancel,
            progress: running.and_then(|r| r.progress.clone()),
            outcome: self.outcome.clone(),
            outcome_kind: self.outcome_kind,
        }
    }
}

/// The cancellation boundary between checking a draft and committing its import.
pub(crate) struct ImportCheck<'a> {
    manager: &'a GenerationManager,
    cancel: CancelToken,
}

impl ImportCheck<'_> {
    pub(crate) fn cancel_token(&self) -> &CancelToken {
        &self.cancel
    }

    /// Returns whether the check reached the commit phase before a cancellation request.
    pub(crate) fn begin_import(&self) -> bool {
        let mut inner = lock(&self.manager.inner);
        if self.cancel.is_cancelled() {
            return false;
        }
        if let Some(running) = inner.running.as_mut() {
            running.cancel = None;
            inner.revision += 1;
        }
        true
    }
}

pub(crate) enum DataReplacement<'a> {
    Restore(&'a str),
    Import(&'a Path),
}

impl DataReplacement<'_> {
    fn action(&self) -> &'static str {
        match self {
            Self::Restore(_) => "restaurar",
            Self::Import(_) => "importar",
        }
    }
}

/// Replaces the study data once the backup service has a turn and no AI work is running.
pub(crate) fn replace_study_data(
    service: &BackupService,
    db: &Database,
    paths: &AppPaths,
    generation: &GenerationManager,
    on_demand: &OnDemand,
    replacement: DataReplacement<'_>,
    now: OffsetDateTime,
) -> AppResult<()> {
    let ready = || {
        if generation.is_running() || on_demand.is_running() {
            let action = replacement.action();
            return Err(AppError::Invalid(format!(
                "Há uma consulta à IA em andamento. Espere ou cancele antes de {action}."
            )));
        }
        Ok(())
    };
    match replacement {
        DataReplacement::Restore(name) => service.restore(db, paths, name, now, ready)?,
        DataReplacement::Import(file) => service.import(db, paths, file, now, ready)?,
    }
    on_demand.clear_proposals();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::runner::ProgressKind;
    use std::sync::mpsc;
    use std::time::Duration;

    const STARTED_AT: &str = "2026-11-20T12:00:00.000Z";

    #[test]
    fn generation_status_reports_an_empty_stage_list_when_idle() {
        let status = GenerationManager::default().status();
        assert_eq!(status.stages, []);
        assert!(!status.can_cancel);
        assert!(!status.cancel_discards_draft);
    }

    #[test]
    fn generation_status_reports_ordered_stages_and_explains_the_cancel_effect() {
        let manager = GenerationManager::default();
        manager
            .reserve(GenerationKind::Generate, "now".into())
            .unwrap();

        let prepared = manager.status();
        assert_eq!(
            prepared
                .stages
                .iter()
                .map(|step| step.id)
                .collect::<Vec<_>>(),
            [
                GenerationStage::Generating,
                GenerationStage::Checking,
                GenerationStage::Resolving
            ]
        );
        assert!(!prepared.cancel_discards_draft);
        assert!(prepared.can_cancel);
        manager
            .perform(
                &|status| assert!(status.cancel_discards_draft),
                |progress| {
                    progress(Progress::starting(
                        GenerationStage::Checking,
                        ProgressKind::Started,
                        "x",
                    ));
                    Ok(())
                },
            )
            .unwrap();
    }

    #[test]
    fn a_recheck_reports_only_checking_and_resolving_stages() {
        let manager = GenerationManager::default();
        manager
            .reserve(GenerationKind::Recheck, "now".into())
            .unwrap();

        assert_eq!(
            manager
                .status()
                .stages
                .iter()
                .map(|step| step.id)
                .collect::<Vec<_>>(),
            [GenerationStage::Checking, GenerationStage::Resolving]
        );
        manager
            .perform(
                &|status| assert!(!status.cancel_discards_draft),
                |progress| {
                    progress(Progress::starting(
                        GenerationStage::Checking,
                        ProgressKind::Started,
                        "x",
                    ));
                    Ok(())
                },
            )
            .unwrap();
    }

    #[test]
    fn a_delayed_completion_has_an_older_revision_than_the_next_run() {
        let manager = GenerationManager::default();
        let (finishing, finish_started) = mpsc::channel();
        let (release_finish, wait_to_finish) = mpsc::channel();
        let (emit_first, first_event) = mpsc::channel();
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
                    Ok("first".into())
                },
                move |status| {
                    if !status.running {
                        finishing.send(()).unwrap();
                        wait_to_finish.recv().unwrap();
                    }
                    emit_first.send(status.clone()).unwrap();
                },
            )
            .unwrap();
        let first = first_event.recv_timeout(Duration::from_secs(5)).unwrap();
        finish_started.recv_timeout(Duration::from_secs(5)).unwrap();

        let (emit, next_event) = mpsc::channel();
        let (release_next, wait_for_release) = mpsc::channel();
        manager
            .start(
                GenerationKind::Generate,
                STARTED_AT.into(),
                move |_, progress| {
                    progress(Progress::starting(
                        GenerationStage::Checking,
                        ProgressKind::Started,
                        "x",
                    ));
                    wait_for_release.recv().unwrap();
                    Ok("second".into())
                },
                move |status| emit.send(status.clone()).unwrap(),
            )
            .unwrap();
        let next = next_event.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(manager.status(), next);

        release_finish.send(()).unwrap();
        let completion = first_event.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(!completion.running);
        assert!(next.running);
        assert_eq!(first.started_at, next.started_at);
        assert!(completion.revision < next.revision);
        assert!(first.run_revision < completion.run_revision);
        assert!(completion.run_revision < next.run_revision);
        assert!(next.run_revision < next.revision);
        release_next.send(()).unwrap();
        assert!(
            !next_event
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .running
        );
    }

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
