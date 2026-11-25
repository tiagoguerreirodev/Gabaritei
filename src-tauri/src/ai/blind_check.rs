//! The blind check (spec §3.2 steps 3–4): a second model answers the questions without the key;
//! disagreements go back to the generator once; what still disagrees is flagged `disputed`.

use std::collections::{HashMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::draft::{self, Draft, DraftStatus, GenerationInfo};
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

/// The stored result of the blind check of a draft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlindCheck {
    pub verdicts: Vec<Verdict>,
    /// Set when the check couldn't be completed; there are no verdicts then.
    pub failed: Option<String>,
    pub info: Option<GenerationInfo>,
    /// The check is running now. The draft cannot be imported or discarded until it ends.
    #[serde(default)]
    pub pending: bool,
}

/// The record a draft carries while its blind check runs.
pub fn pending_record() -> serde_json::Value {
    serde_json::to_value(BlindCheck {
        verdicts: Vec::new(),
        failed: None,
        info: None,
        pending: true,
    })
    .expect("a blind check serializes")
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
        return Err(invalid(
            "A conferência não respondeu todas as questões. Tente conferir de novo.",
        ));
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

/// The pipeline's blind-check stage on a saved draft. Saves the verdicts (and any revised
/// questions) into the draft and returns them.
pub fn run(
    db: &Database,
    secrets: &Secrets,
    runtime: &GenerationRuntime<'_>,
    draft_id: &DraftId,
    attachments: &[std::path::PathBuf],
    cancel: &CancelToken,
    progress: &mut dyn FnMut(Progress),
) -> AppResult<BlindCheck> {
    let (settings, draft) =
        db.with(|conn| Ok((settings::load(conn, secrets)?, draft::load(conn, draft_id)?)))?;
    let checker = draft.request.checker.clone();
    let generator = draft.request.generator.clone();
    let request: &GenerateRequest = &draft.request.request;
    let started_at = (runtime.now)();
    let began = (runtime.monotonic_time)();
    let mut package = draft.package.clone();

    progress(Progress::starting(
        GenerationStage::Checking,
        ProgressKind::Started,
        "Conferindo às cegas…",
    ));
    let all: Vec<&PackageQuestion> = package.questions.iter().collect();
    let (first, first_output) = check_questions(
        &settings,
        secrets,
        runtime.clis,
        &checker,
        &all,
        cancel,
        progress,
    )?;
    let mut cost = first_output.cost_usd;

    let objections: Vec<&PackageQuestion> = package
        .questions
        .iter()
        .filter(|q| first[&q.id].choice != q.answer_id)
        .collect();
    let objected: HashSet<&str> = objections.iter().map(|q| q.id.as_str()).collect();

    // Round two: the generator fixes or defends, then the checker re-checks what was revised.
    let mut revised_by_id: HashMap<String, PackageQuestion> = HashMap::new();
    let mut second: HashMap<String, CheckerAnswer> = HashMap::new();
    if !objections.is_empty() {
        progress(Progress::starting(
            GenerationStage::Resolving,
            ProgressKind::Thinking,
            "Resolvendo as contestações…",
        ));
        let items: Vec<Objection<'_>> = objections
            .iter()
            .map(|question| Objection {
                question,
                checker_choice: &first[&question.id].choice,
                checker_reason: &first[&question.id].reason,
            })
            .collect();
        let (resolutions, output): (DisputeResolutions, _) = ask(
            &settings,
            secrets,
            runtime.clis,
            &generator,
            Task {
                schema: schema_of::<DisputeResolutions>(),
                sources: Sources {
                    remnote: request.sources.remnote,
                    web: request.sources.web,
                },
                instructions: prompts::dispute_instructions(),
                prompt: prompts::dispute_prompt(&items),
                // The generator answers the objections with the files it wrote from.
                attachments: attachments.to_vec(),
            },
            cancel,
            progress,
        )?;
        cost = sum_cost(cost, output.cost_usd);
        revised_by_id = revisions(resolutions, &objected);
        if !revised_by_id.is_empty() {
            progress(Progress::starting(
                GenerationStage::Resolving,
                ProgressKind::Thinking,
                "Reconferindo as questões revisadas…",
            ));
            let mut ids: Vec<&String> = revised_by_id.keys().collect();
            ids.sort();
            let revised: Vec<&PackageQuestion> = ids.iter().map(|id| &revised_by_id[*id]).collect();
            let (answers, output) = check_questions(
                &settings,
                secrets,
                runtime.clis,
                &checker,
                &revised,
                cancel,
                progress,
            )?;
            cost = sum_cost(cost, output.cost_usd);
            second = answers;
        }
    }

    let verdicts: Vec<Verdict> = package
        .questions
        .iter()
        .map(|question| {
            let first = &first[&question.id];
            let objected = objected.contains(question.id.as_str());
            let (state, last) = if !objected {
                (VerdictState::Agreed, first)
            } else if let (Some(revised), Some(again)) =
                (revised_by_id.get(&question.id), second.get(&question.id))
            {
                let state = if again.choice == revised.answer_id {
                    VerdictState::Fixed
                } else {
                    VerdictState::Disputed
                };
                (state, again)
            } else {
                (VerdictState::Disputed, first)
            };
            Verdict {
                question_id: question.id.clone(),
                state,
                first_choice: first.choice.clone(),
                final_choice: last.choice.clone(),
                confidence: last.confidence,
                reason: last.reason.clone(),
                objected,
            }
        })
        .collect();

    for question in &mut package.questions {
        if let Some(revised) = revised_by_id.remove(&question.id) {
            *question = revised;
        }
    }
    let elapsed = (runtime.monotonic_time)().saturating_sub(began);
    let result = BlindCheck {
        verdicts,
        failed: None,
        pending: false,
        info: Some(GenerationInfo {
            agent: checker.agent,
            model: checker.model.clone(),
            effort: checker.effort,
            started_at,
            duration_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
            cost_usd: cost,
        }),
    };
    db.transaction(|tx| {
        draft::set_package(tx, draft_id, &package)?;
        draft::set_blind_check(tx, draft_id, &serde_json::to_value(&result)?)
    })?;
    Ok(result)
}

/// Records that the check couldn't be completed, so the preview can say so.
pub fn record_failure(db: &Database, draft_id: &DraftId, message: &str) -> AppResult<()> {
    let failed = BlindCheck {
        verdicts: Vec::new(),
        failed: Some(message.to_owned()),
        info: None,
        pending: false,
    };
    db.with(|conn| draft::set_blind_check(conn, draft_id, &serde_json::to_value(failed)?))
}

/// The stored blind check of a draft, if it has one.
pub fn stored(draft: &Draft) -> AppResult<Option<BlindCheck>> {
    draft
        .blind_check
        .as_ref()
        .map(|value| serde_json::from_value(value.clone()).map_err(AppError::from))
        .transpose()
}

/// The whole pipeline: generate, validate, blind-check, resolve disputes once, save the draft.
/// Returns the draft's ID. A check that fails (not a cancel) leaves the draft saved with the
/// failure recorded; a cancel during the check discards the draft.
pub fn generate_and_check(
    db: &Database,
    secrets: &Secrets,
    runtime: &GenerationRuntime<'_>,
    files_dir: &std::path::Path,
    request: GenerateRequest,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(Progress),
) -> AppResult<DraftId> {
    progress(Progress::starting(
        GenerationStage::Generating,
        ProgressKind::Started,
        "Gerando as questões…",
    ));
    let originals: Vec<std::path::PathBuf> = request
        .sources
        .attachments
        .iter()
        .map(std::path::PathBuf::from)
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
    match run(
        db,
        secrets,
        runtime,
        &draft.id,
        &attachments,
        cancel,
        progress,
    ) {
        Ok(_) => Ok(draft.id),
        Err(_) if cancel.is_cancelled() => {
            draft_files::remove(files_dir, &draft.id);
            db.with(|conn| draft::set_status(conn, &draft.id, DraftStatus::Discarded))?;
            Err(invalid("A execução foi cancelada."))
        }
        Err(error) => {
            record_failure(db, &draft.id, &error.to_string())?;
            Ok(draft.id)
        }
    }
}

/// Re-runs the blind check of a draft that has none or whose check failed. The dispute round
/// gets the attachments kept for the draft (see [`draft_files`]); if they are gone (an older
/// draft, or one restored from a backup) it runs without them and says so in the progress.
pub fn recheck_draft(
    db: &Database,
    secrets: &Secrets,
    runtime: &GenerationRuntime<'_>,
    files_dir: &std::path::Path,
    draft_id: &DraftId,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(Progress),
) -> AppResult<DraftId> {
    let wanted = db.transaction(|tx| {
        let draft = draft::load(tx, draft_id)?;
        if draft.status != DraftStatus::Review {
            return Err(invalid("Este pacote já foi importado ou descartado."));
        }
        if stored(&draft)?.is_some_and(|check| check.pending) {
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
    match run(
        db,
        secrets,
        runtime,
        draft_id,
        &attachments,
        cancel,
        progress,
    ) {
        Ok(_) => Ok(draft_id.to_owned()),
        Err(_) if cancel.is_cancelled() => {
            record_failure(db, draft_id, "A conferência foi cancelada.")?;
            Err(invalid("A execução foi cancelada."))
        }
        Err(error) => {
            record_failure(db, draft_id, &error.to_string())?;
            Ok(draft_id.to_owned())
        }
    }
}

/// After a restart, a check that was running can never finish: mark it as interrupted so the
/// draft can be imported, discarded or checked again.
pub fn recover_interrupted(conn: &rusqlite::Connection) -> AppResult<()> {
    let failed = serde_json::to_string(&BlindCheck {
        verdicts: Vec::new(),
        failed: Some("A conferência foi interrompida antes de terminar.".into()),
        info: None,
        pending: false,
    })?;
    conn.execute(
        "UPDATE drafts SET blind_check = ?1
         WHERE status = ?2 AND json_extract(blind_check, '$.pending') = 1",
        rusqlite::params![failed, DraftStatus::Review],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
