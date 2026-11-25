//! The blind check (spec §3.2 steps 3–4): a second model answers the questions without the key;
//! disagreements go back to the generator once; what still disagrees is flagged `disputed`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::draft::{self, DraftRequest, DraftStatus, GenerationInfo};
use super::draft_files;
use super::generate::{GenerateRequest, GenerationRuntime};
use super::jobs::GenerationStage;
use super::prompts::{self, CheckerView, Objection};
use super::roles::RoleConfig;
use super::runner::{CancelToken, Progress, ProgressKind};
use super::secrets::Secrets;
use super::settings::{self, AiSettings, ResolvedClis, Sources};
use super::task::{Task, ask};
use crate::bank::PackageQuestion;
use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::ids::DraftId;

/// How sure the checker is of its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

/// The checker's answer to one question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CheckerAnswer {
    // Checker answers, disputes and verdicts use the package's own question IDs.
    pub question_id: String,
    /// ID of the option the checker chose.
    pub choice: String,
    pub confidence: Confidence,
    pub reason: String,
}

/// What the checker returns (the root of a structured answer must be an object).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Conferência às cegas")]
pub struct CheckerAnswers {
    pub verdicts: Vec<CheckerAnswer>,
}

/// What the generator decides about an objection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DisputeAction {
    Revise,
    Defend,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DisputeResolution {
    pub question_id: String,
    pub action: DisputeAction,
    /// The corrected question when `action` is `revise`; empty when it is `defend`.
    pub revised: Vec<PackageQuestion>,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Resolução das contestações")]
pub struct DisputeResolutions {
    pub resolutions: Vec<DisputeResolution>,
}

/// Where one question ended up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum VerdictState {
    /// The checker chose the generator's key.
    Agreed,
    /// The checker disagreed, the generator revised the question, and the checker then agreed.
    Fixed,
    /// The checker still disagrees (the generator defended it, or the revision didn't convince).
    Disputed,
}

/// The verdict on one question. The reason can reveal the answer: backend only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    pub question_id: String,
    pub state: VerdictState,
    /// What the checker chose in the first round.
    pub first_choice: String,
    /// What the checker chose last.
    pub final_choice: String,
    pub confidence: Confidence,
    pub reason: String,
    /// Whether the generator was asked about this question.
    pub objected: bool,
}

/// The result of the blind check of a draft.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(from = "BlindCheckRecord<Vec<Verdict>, String, GenerationInfo>")]
pub enum BlindCheck {
    Pending,
    Failed(String),
    Done {
        verdicts: Vec<Verdict>,
        /// Old checks can have verdicts without provenance.
        info: Option<GenerationInfo>,
    },
}

impl BlindCheck {
    pub(super) fn is_pending(&self) -> bool {
        matches!(self, Self::Pending)
    }

    pub(super) fn verdicts(&self) -> &[Verdict] {
        match self {
            Self::Done { verdicts, .. } => verdicts,
            Self::Pending | Self::Failed(_) => &[],
        }
    }

    pub(super) fn failure(&self) -> Option<&str> {
        match self {
            Self::Failed(message) => Some(message),
            Self::Pending | Self::Done { .. } => None,
        }
    }
}

/// Preserve the original draft JSON shape, including the pending flag read during recovery.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlindCheckRecord<V, F, I> {
    verdicts: V,
    failed: Option<F>,
    info: Option<I>,
    #[serde(default)]
    pending: bool,
}

impl From<BlindCheckRecord<Vec<Verdict>, String, GenerationInfo>> for BlindCheck {
    fn from(record: BlindCheckRecord<Vec<Verdict>, String, GenerationInfo>) -> Self {
        if record.pending {
            Self::Pending
        } else if let Some(message) = record.failed {
            Self::Failed(message)
        } else {
            Self::Done {
                verdicts: record.verdicts,
                info: record.info,
            }
        }
    }
}

impl Serialize for BlindCheck {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        BlindCheckRecord {
            verdicts: self.verdicts(),
            failed: self.failure(),
            info: match self {
                Self::Done { info, .. } => info.as_ref(),
                Self::Pending | Self::Failed(_) => None,
            },
            pending: self.is_pending(),
        }
        .serialize(serializer)
    }
}

/// The record a draft carries while its blind check runs.
pub fn pending_record() -> BlindCheck {
    BlindCheck::Pending
}

use super::invalid;

/// Asks the checker about `questions` and checks that it answered each one, once, with an
/// option that exists.
fn check_questions(
    settings: &AiSettings,
    secrets: &Secrets,
    clis: &ResolvedClis,
    checker: &RoleConfig,
    questions: &[&PackageQuestion],
    cancel: &CancelToken,
    progress: &mut dyn FnMut(Progress),
) -> AppResult<(HashMap<String, CheckerAnswer>, super::runner::JobOutput)> {
    let views: Vec<CheckerView> = questions.iter().map(|q| CheckerView::from(*q)).collect();
    let (answers, output): (CheckerAnswers, _) = ask(
        settings,
        secrets,
        clis,
        checker,
        Task {
            schema: schema_of::<CheckerAnswers>(),
            sources: Sources::default(),
            instructions: prompts::blind_check_instructions(),
            prompt: prompts::blind_check_prompt(&views),
            attachments: Vec::new(),
        },
        cancel,
        progress,
    )?;
    let mut by_id: HashMap<String, CheckerAnswer> = HashMap::new();
    for answer in answers.verdicts {
        let question = questions
            .iter()
            .find(|q| q.id == answer.question_id)
            .ok_or_else(|| {
                invalid("A conferência respondeu uma questão que não existe no pacote.")
            })?;
        if !question.options.iter().any(|o| o.id == answer.choice) {
            return Err(invalid(
                "A conferência marcou uma alternativa que não existe. Tente conferir de novo.",
            ));
        }
        if by_id.insert(answer.question_id.clone(), answer).is_some() {
            return Err(invalid(
                "A conferência respondeu a mesma questão duas vezes.",
            ));
        }
    }
    if by_id.len() != questions.len() {
        return Err(missing_checker_answer());
    }
    Ok((by_id, output))
}

pub(super) fn schema_of<T: JsonSchema>() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(T)).expect("schema serializes")
}

fn sum_cost(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
    }
}

/// The generator's answer to the objections, as questions to swap into the package.
/// A `revise` without exactly one usable replacement counts as a defence.
fn revisions(
    resolutions: DisputeResolutions,
    objected: &HashSet<&str>,
) -> HashMap<String, PackageQuestion> {
    resolutions
        .resolutions
        .into_iter()
        .filter(|r| r.action == DisputeAction::Revise && objected.contains(r.question_id.as_str()))
        .filter_map(|r| {
            let mut revised = r.revised;
            (revised.len() == 1 && revised[0].id == r.question_id)
                .then(|| (r.question_id, revised.remove(0)))
        })
        .collect()
}

/// The pipeline's blind-check stage on a saved draft. Saves the verdicts and any revisions.
pub fn run(
    db: &Database,
    secrets: &Secrets,
    runtime: &GenerationRuntime<'_>,
    draft_id: &DraftId,
    attachments: &[PathBuf],
    cancel: &CancelToken,
    progress: &mut dyn FnMut(Progress),
) -> AppResult<BlindCheck> {
    let (settings, draft) =
        db.with(|conn| Ok((settings::load(conn, secrets)?, draft::load(conn, draft_id)?)))?;
    let started_at = (runtime.now)();
    let began = (runtime.monotonic_time)();
    let mut package = draft.package;
    let mut check = CheckRun {
        settings: &settings,
        secrets,
        clis: runtime.clis,
        request: &draft.request,
        attachments,
        cancel,
        progress,
    };

    (check.progress)(Progress::starting(
        GenerationStage::Checking,
        ProgressKind::Started,
        "Conferindo às cegas…",
    ));
    let all: Vec<_> = package.questions.iter().collect();
    let (first, output) = check.ask_checker(&all)?;
    let objections = objections(&package.questions, &first)?;
    let mut round = check.resolve(&objections, output.cost_usd)?;
    let verdicts = verdicts(&package.questions, &first, &round)?;

    for question in &mut package.questions {
        if let Some(revised) = round.revisions.remove(&question.id) {
            *question = revised;
        }
    }
    let elapsed = (runtime.monotonic_time)().saturating_sub(began);
    let result = BlindCheck::Done {
        verdicts,
        info: Some(GenerationInfo {
            agent: draft.request.checker.agent,
            model: draft.request.checker.model,
            effort: draft.request.checker.effort,
            started_at,
            duration_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
            cost_usd: round.cost,
        }),
    };
    db.transaction(|tx| {
        draft::set_package(tx, draft_id, &package)?;
        draft::set_blind_check(tx, draft_id, &result)
    })?;
    Ok(result)
}

struct CheckRun<'a> {
    settings: &'a AiSettings,
    secrets: &'a Secrets,
    clis: &'a ResolvedClis,
    request: &'a DraftRequest,
    attachments: &'a [PathBuf],
    cancel: &'a CancelToken,
    progress: &'a mut dyn FnMut(Progress),
}

#[derive(Default)]
struct DisputeRound {
    revisions: HashMap<String, PackageQuestion>,
    answers: HashMap<String, CheckerAnswer>,
    cost: Option<f64>,
}

impl CheckRun<'_> {
    fn ask_checker(
        &mut self,
        questions: &[&PackageQuestion],
    ) -> AppResult<(HashMap<String, CheckerAnswer>, super::runner::JobOutput)> {
        check_questions(
            self.settings,
            self.secrets,
            self.clis,
            &self.request.checker,
            questions,
            self.cancel,
            self.progress,
        )
    }

    fn resolve(
        &mut self,
        objections: &[Objection<'_>],
        cost: Option<f64>,
    ) -> AppResult<DisputeRound> {
        if objections.is_empty() {
            return Ok(DisputeRound {
                cost,
                ..DisputeRound::default()
            });
        }
        (self.progress)(Progress::starting(
            GenerationStage::Resolving,
            ProgressKind::Thinking,
            "Resolvendo as contestações…",
        ));
        let (resolutions, output): (DisputeResolutions, _) = ask(
            self.settings,
            self.secrets,
            self.clis,
            &self.request.generator,
            Task {
                schema: schema_of::<DisputeResolutions>(),
                sources: Sources {
                    remnote: self.request.request.sources.remnote,
                    web: self.request.request.sources.web,
                },
                instructions: prompts::dispute_instructions(),
                prompt: prompts::dispute_prompt(objections),
                // The generator answers the objections with the files it wrote from.
                attachments: self.attachments.to_vec(),
            },
            self.cancel,
            self.progress,
        )?;
        let objected = objections
            .iter()
            .map(|item| item.question.id.as_str())
            .collect();
        let revisions = revisions(resolutions, &objected);
        let mut round = DisputeRound {
            revisions,
            cost: sum_cost(cost, output.cost_usd),
            ..DisputeRound::default()
        };
        if !round.revisions.is_empty() {
            (self.progress)(Progress::starting(
                GenerationStage::Resolving,
                ProgressKind::Thinking,
                "Reconferindo as questões revisadas…",
            ));
            let mut questions: Vec<_> = round.revisions.values().collect();
            questions.sort_unstable_by(|a, b| a.id.cmp(&b.id));
            let (answers, output) = self.ask_checker(&questions)?;
            round.answers = answers;
            round.cost = sum_cost(round.cost, output.cost_usd);
        }
        Ok(round)
    }
}

fn objections<'a>(
    questions: &'a [PackageQuestion],
    first: &'a HashMap<String, CheckerAnswer>,
) -> AppResult<Vec<Objection<'a>>> {
    let mut result = Vec::new();
    for question in questions {
        let answer = first.get(&question.id).ok_or_else(missing_checker_answer)?;
        if answer.choice != question.answer_id {
            result.push(Objection {
                question,
                checker_choice: &answer.choice,
                checker_reason: &answer.reason,
            });
        }
    }
    Ok(result)
}

fn verdicts(
    questions: &[PackageQuestion],
    first: &HashMap<String, CheckerAnswer>,
    round: &DisputeRound,
) -> AppResult<Vec<Verdict>> {
    questions
        .iter()
        .map(|question| {
            let first = first.get(&question.id).ok_or_else(missing_checker_answer)?;
            let objected = first.choice != question.answer_id;
            let (state, last) = if !objected {
                (VerdictState::Agreed, first)
            } else if let (Some(revised), Some(again)) = (
                round.revisions.get(&question.id),
                round.answers.get(&question.id),
            ) {
                let state = if again.choice == revised.answer_id {
                    VerdictState::Fixed
                } else {
                    VerdictState::Disputed
                };
                (state, again)
            } else {
                (VerdictState::Disputed, first)
            };
            Ok(Verdict {
                question_id: question.id.clone(),
                state,
                first_choice: first.choice.clone(),
                final_choice: last.choice.clone(),
                confidence: last.confidence,
                reason: last.reason.clone(),
                objected,
            })
        })
        .collect()
}

fn missing_checker_answer() -> AppError {
    invalid("A conferência não respondeu todas as questões. Tente conferir de novo.")
}

/// Records that the check couldn't be completed, so the preview can say so.
pub fn record_failure(db: &Database, draft_id: &DraftId, message: &str) -> AppResult<()> {
    let failed = BlindCheck::Failed(message.to_owned());
    db.with(|conn| draft::set_blind_check(conn, draft_id, &failed))
}

/// The whole pipeline: generate, validate, blind-check, resolve disputes once, save the draft.
/// Returns the draft's ID. A check that fails (not a cancel) leaves the draft saved with the
/// failure recorded; a cancel during the check discards the draft.
pub fn generate_and_check(
    db: &Database,
    secrets: &Secrets,
    runtime: &GenerationRuntime<'_>,
    files_dir: &Path,
    request: GenerateRequest,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(Progress),
) -> AppResult<DraftId> {
    progress(Progress::starting(
        GenerationStage::Generating,
        ProgressKind::Started,
        "Gerando as questões…",
    ));
    let originals: Vec<PathBuf> = request
        .sources
        .attachments
        .iter()
        .map(PathBuf::from)
        .collect();
    let draft = super::generate::generate_draft(db, secrets, runtime, request, cancel, progress)?;
    // From here a cancel discards the draft, so say so before anything else happens.
    progress(Progress::starting(
        GenerationStage::Checking,
        ProgressKind::Started,
        "Conferindo às cegas…",
    ));
    // Keep copies for the life of the draft so a retry can give the generator the same files.
    // If copying fails (the file moved meanwhile), this run still uses the originals.
    let attachments = draft_files::keep(files_dir, &draft.id, &originals).unwrap_or(originals);
    let result = run(
        db,
        secrets,
        runtime,
        &draft.id,
        &attachments,
        cancel,
        progress,
    );
    finish_check(
        db,
        files_dir,
        &draft.id,
        result,
        cancel,
        CheckCancellation::DiscardDraft,
    )
}

/// Re-runs the blind check of a draft that has none or whose check failed. The dispute round
/// gets the attachments kept for the draft (see [`draft_files`]); if they are gone (an older
/// draft, or one restored from a backup) it runs without them and says so in the progress.
pub fn recheck_draft(
    db: &Database,
    secrets: &Secrets,
    runtime: &GenerationRuntime<'_>,
    files_dir: &Path,
    draft_id: &DraftId,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(Progress),
) -> AppResult<DraftId> {
    let wanted = db.transaction(|tx| {
        let draft = draft::load(tx, draft_id)?;
        if draft.status != DraftStatus::Review {
            return Err(invalid("Este pacote já foi importado ou descartado."));
        }
        if draft
            .blind_check
            .as_ref()
            .is_some_and(BlindCheck::is_pending)
        {
            return Err(invalid("A conferência deste pacote já está em andamento."));
        }
        draft::set_blind_check(tx, draft_id, &pending_record())?;
        Ok(draft.request.request.sources.attachments.len())
    })?;
    let attachments = draft_files::load(files_dir, draft_id).unwrap_or_default();
    if attachments.len() != wanted {
        // Drafts from before attachments were kept, or restored from a backup, have no copies.
        progress(Progress::new(
            ProgressKind::Thinking,
            "Os anexos originais não estão mais guardados; a contestação segue sem eles.",
        ));
    }
    let result = run(
        db,
        secrets,
        runtime,
        draft_id,
        &attachments,
        cancel,
        progress,
    );
    finish_check(
        db,
        files_dir,
        draft_id,
        result,
        cancel,
        CheckCancellation::KeepDraft,
    )
}

enum CheckCancellation {
    DiscardDraft,
    KeepDraft,
}

fn finish_check(
    db: &Database,
    files_dir: &Path,
    id: &DraftId,
    result: AppResult<BlindCheck>,
    cancel: &CancelToken,
    cancellation: CheckCancellation,
) -> AppResult<DraftId> {
    match result {
        Ok(_) => Ok(id.to_owned()),
        Err(_) if cancel.is_cancelled() => {
            match cancellation {
                CheckCancellation::DiscardDraft => {
                    draft_files::remove(files_dir, id);
                    db.with(|conn| draft::set_status(conn, id, DraftStatus::Discarded))?;
                }
                CheckCancellation::KeepDraft => {
                    record_failure(db, id, "A conferência foi cancelada.")?;
                }
            }
            Err(invalid("A execução foi cancelada."))
        }
        Err(error) => {
            record_failure(db, id, &error.to_string())?;
            Ok(id.to_owned())
        }
    }
}

/// After a restart, a check that was running can never finish: mark it as interrupted so the
/// draft can be imported, discarded or checked again.
pub fn recover_interrupted(conn: &rusqlite::Connection) -> AppResult<()> {
    let failed = serde_json::to_string(&BlindCheck::Failed(
        "A conferência foi interrompida antes de terminar.".into(),
    ))?;
    conn.execute(
        "UPDATE drafts SET blind_check = ?1
         WHERE status = ?2 AND json_extract(blind_check, '$.pending') = 1",
        rusqlite::params![failed, DraftStatus::Review],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
