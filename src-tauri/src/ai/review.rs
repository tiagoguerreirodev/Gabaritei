//! The spoiler-free view of a draft and what the user can do with it (spec §2 decisions 10–11,
//! §3.2 "Preview").
//!
//! [`DraftSummary`] is the only draft type the frontend gets. It carries counts, validation
//! problems and one state per question. Only a *flagged* question (the blind check still
//! disputes it, or it has a validation problem) also carries its stem and options, so the user can
//! judge it. No key, explanation, reference or checker reason ever appears.

use std::path::Path;

use rusqlite::{Connection, Transaction, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::agent::{Agent, Effort};
use super::blind_check::{BlindCheck, VerdictState};
use super::draft::{self, Draft, DraftStatus};
use super::draft_files;
use super::generate::{DraftCheck, DraftProblem, check_draft, draft_inputs};
use super::jobs::{GenerationManager, GenerationStatus};
use super::runner::{CancelToken, Progress};
use crate::bank::catalog::CollectionKind;
use crate::bank::store::{ImportOutcome, import_questions};
use crate::bank::{PackageQuestion, QuestionType, TopicPath};
use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::ids::{self, CollectionId, DraftId, QuestionId};

/// Where a question stands after the blind check, as one line of the preview ("Q3 · contestada").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LineState {
    /// The checker chose the same answer as the key.
    Agreed,
    /// The checker disagreed, the generator revised the question, and then the checker agreed.
    Fixed,
    /// The checker still disagrees.
    Disputed,
    /// No blind check yet (it failed or hasn't run).
    Unchecked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QuestionLine {
    /// 1-based, as in "Q3".
    pub position: u32,
    pub state: LineState,
    /// Whether the question has a validation problem of its own.
    pub has_problem: bool,
    /// The stem and options, only when the question is flagged; `None` otherwise.
    pub content: Option<FlaggedContent>,
}

/// What the user may read of a flagged question. Deliberately has no field for the key, the
/// explanation or the references: adding one here would be a spoiler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FlaggedContent {
    #[serde(rename = "type")]
    #[ts(rename = "type")]
    pub kind: QuestionType,
    pub stem: String,
    pub options: Vec<FlaggedOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FlaggedOption {
    pub id: String,
    pub text: String,
}

/// Agent, model and effort that did one stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StageLabel {
    pub agent: Agent,
    pub model: String,
    pub effort: Effort,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DraftSummary {
    pub id: DraftId,
    pub status: DraftStatus,
    pub created_at: String,
    pub topic: TopicPath,
    pub count: u32,
    pub multiple: u32,
    pub boolean: u32,
    pub requested_format: Option<QuestionType>,
    pub board: Option<String>,
    pub generator: StageLabel,
    pub checker: StageLabel,
    pub duration_ms: u64,
    pub cost_usd: Option<f64>,
    pub lines: Vec<QuestionLine>,
    pub problems: Vec<DraftProblem>,
    /// Questions already in the bank (they merge instead of duplicating).
    pub duplicates: u32,
    pub agreed: u32,
    pub fixed: u32,
    pub disputed: u32,
    pub unchecked: u32,
    pub confirmed: u32,
    pub needs_check: bool,
    /// Why the blind check could not complete, if it failed.
    pub check_failed: Option<String>,
    /// The blind check is still running: the package can't be imported or discarded yet.
    pub checking: bool,
}

/// A draft in the "waiting for review" list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DraftListItem {
    pub id: DraftId,
    pub topic: TopicPath,
    pub count: u32,
    pub created_at: String,
    pub disputed: u32,
    /// Its blind check is still running.
    pub checking: bool,
}

/// Which questions to import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ImportMode {
    /// Every question. Refused while the package has problems or unchecked questions (the
    /// Revisar page runs the blind check first).
    All,
    /// Only the ones the checker agreed with (directly or after a fix) and that have no problems.
    Confirmed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportResult {
    pub collection_id: CollectionId,
    pub title: String,
    /// New questions added to the bank.
    pub imported: u32,
    /// Questions that were already in the bank and are now in the new collection.
    pub merged: u32,
    /// Questions left out of the import.
    pub skipped: u32,
}

/// The result of importing every question, with a blind check first when needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum ImportAllOutcome {
    Imported { result: ImportResult },
    Pending { message: String },
}

struct ReviewStates {
    lines: Vec<LineState>,
    failed: Option<String>,
    checking: bool,
}

impl ReviewStates {
    fn needs_check(&self) -> bool {
        self.failed.is_some() || self.lines.contains(&LineState::Unchecked)
    }
}

fn states(draft: &Draft) -> AppResult<ReviewStates> {
    let check = draft.blind_check.as_ref();
    let failed = check.and_then(BlindCheck::failure).map(str::to_owned);
    let pending = check.is_some_and(BlindCheck::is_pending);
    let lines = draft
        .package
        .questions
        .iter()
        .map(|question| {
            check
                .and_then(|c| c.verdicts().iter().find(|v| v.question_id == question.id))
                .map_or(LineState::Unchecked, |verdict| match verdict.state {
                    VerdictState::Agreed => LineState::Agreed,
                    VerdictState::Fixed => LineState::Fixed,
                    VerdictState::Disputed => LineState::Disputed,
                })
        })
        .collect();
    Ok(ReviewStates {
        lines,
        failed,
        checking: pending,
    })
}

/// Copies the stem and options of a flagged question, and nothing else.
fn flagged_content(question: &PackageQuestion) -> FlaggedContent {
    FlaggedContent {
        kind: question.kind,
        stem: question.stem.clone(),
        options: question
            .options
            .iter()
            .map(|option| FlaggedOption {
                id: option.id.clone(),
                text: option.text.clone(),
            })
            .collect(),
    }
}

fn count_u32(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

fn count(items: &[LineState], wanted: LineState) -> u32 {
    count_u32(items.iter().filter(|s| **s == wanted).count())
}

pub fn summary(conn: &Connection, id: &DraftId) -> AppResult<DraftSummary> {
    let draft = draft::load(conn, id)?;
    let check = check_draft(conn, &draft)?;
    let states = states(&draft)?;
    let agreed = count(&states.lines, LineState::Agreed);
    let fixed = count(&states.lines, LineState::Fixed);
    let request = &draft.request;
    Ok(DraftSummary {
        id: draft.id.clone(),
        status: draft.status,
        created_at: draft.created_at.clone(),
        topic: request.request.topic.clone(),
        count: count_u32(draft.package.questions.len()),
        multiple: count_u32(check.multiple),
        boolean: count_u32(check.boolean),
        requested_format: request.request.format,
        board: request.request.board.clone(),
        generator: StageLabel {
            agent: request.generator.agent,
            model: request.generator.model.clone(),
            effort: request.generator.effort,
        },
        checker: StageLabel {
            agent: request.checker.agent,
            model: request.checker.model.clone(),
            effort: request.checker.effort,
        },
        duration_ms: request.info.duration_ms,
        cost_usd: request.info.cost_usd,
        lines: question_lines(&draft.package.questions, &states.lines, &check),
        problems: check.problems,
        duplicates: count_u32(check.duplicates),
        agreed,
        fixed,
        disputed: count(&states.lines, LineState::Disputed),
        unchecked: count(&states.lines, LineState::Unchecked),
        confirmed: agreed.saturating_add(fixed),
        needs_check: states.needs_check(),
        check_failed: states.failed,
        checking: states.checking,
    })
}

fn question_lines(
    questions: &[PackageQuestion],
    states: &[LineState],
    check: &DraftCheck,
) -> Vec<QuestionLine> {
    questions
        .iter()
        .zip(states)
        .enumerate()
        .map(|(index, (question, state))| {
            let has_problem = check.is_blocked(index);
            QuestionLine {
                position: count_u32(index + 1),
                state: *state,
                has_problem,
                content: (*state == LineState::Disputed || has_problem)
                    .then(|| flagged_content(question)),
            }
        })
        .collect()
}

/// Drafts waiting for review, newest first.
pub fn waiting(conn: &Connection) -> AppResult<Vec<DraftListItem>> {
    let ids: Vec<DraftId> = conn
        .prepare("SELECT id FROM drafts WHERE status = ?1 ORDER BY created_at DESC, id")?
        .query_map([DraftStatus::Review], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    ids.iter()
        .map(|id| {
            let draft = draft::load(conn, id)?;
            let ReviewStates {
                lines: states,
                checking,
                ..
            } = states(&draft)?;
            Ok(DraftListItem {
                id: draft.id.clone(),
                topic: draft.request.request.topic.clone(),
                count: count_u32(draft.package.questions.len()),
                created_at: draft.created_at.clone(),
                disputed: count(&states, LineState::Disputed),
                checking,
            })
        })
        .collect()
}

fn checking_error() -> AppError {
    AppError::Invalid("A conferência às cegas ainda está em andamento. Espere terminar.".into())
}

fn not_waiting() -> AppError {
    AppError::Invalid("Este pacote já foi importado ou descartado.".into())
}

/// Imports a draft's questions as a new generated collection and removes its kept attachments
/// after the transaction commits.
pub fn import(
    db: &Database,
    files_dir: &Path,
    id: &DraftId,
    mode: ImportMode,
    now: &str,
) -> AppResult<ImportResult> {
    let result = db.transaction(|tx| import_into_bank(tx, id, mode, now))?;
    draft_files::remove(files_dir, id);
    Ok(result)
}

/// Checks an unfinished package on the generation slot and imports only after a complete check.
pub(crate) fn import_all(
    db: &Database,
    files_dir: &Path,
    id: &DraftId,
    manager: &GenerationManager,
    now: impl Fn() -> String,
    check: impl FnOnce(&CancelToken, &mut dyn FnMut(Progress)) -> AppResult<()>,
    on_event: impl Fn(&GenerationStatus),
) -> AppResult<ImportAllOutcome> {
    let needs_check = db.with(|conn| {
        let (draft, states) = waiting_draft(conn, id)?;
        if states.needs_check() && !check_draft(conn, &draft)?.problems.is_empty() {
            return Err(import_problems());
        }
        Ok(states.needs_check())
    })?;
    if !needs_check {
        let result = import(db, files_dir, id, ImportMode::All, &now())?;
        return Ok(ImportAllOutcome::Imported { result });
    }

    manager.run_check_then_import(
        now(),
        |run, progress| {
            let checked = check(run.cancel_token(), progress);
            if !run.begin_import() {
                return Ok(ImportAllOutcome::Pending {
                    message: "A conferência não terminou: A conferência foi cancelada. Nada foi importado.".into(),
                });
            }
            checked?;
            on_event(&manager.status());
            import_checked(db, files_dir, id, &now())
        },
        &on_event,
    )
}

fn import_checked(
    db: &Database,
    files_dir: &Path,
    id: &DraftId,
    now: &str,
) -> AppResult<ImportAllOutcome> {
    let outcome = db.transaction(|tx| {
        let (draft, states) = waiting_draft(tx, id)?;
        let check = check_draft(tx, &draft)?;
        if states.needs_check() || !check.problems.is_empty() {
            let message = states.failed.map_or_else(
                || "O pacote ainda tem pendências. Nada foi importado.".into(),
                |failure| format!("A conferência não terminou: {failure} Nada foi importado."),
            );
            return Ok(ImportAllOutcome::Pending { message });
        }
        let result = write_import(tx, &draft, &states.lines, &check, ImportMode::All, now)?;
        Ok(ImportAllOutcome::Imported { result })
    })?;
    if matches!(outcome, ImportAllOutcome::Imported { .. }) {
        draft_files::remove(files_dir, id);
    }
    Ok(outcome)
}

fn waiting_draft(conn: &Connection, id: &DraftId) -> AppResult<(Draft, ReviewStates)> {
    let draft = draft::load(conn, id)?;
    if draft.status != DraftStatus::Review {
        return Err(not_waiting());
    }
    let states = states(&draft)?;
    if states.checking {
        return Err(checking_error());
    }
    Ok((draft, states))
}

fn import_into_bank(
    conn: &Transaction<'_>,
    id: &DraftId,
    mode: ImportMode,
    now: &str,
) -> AppResult<ImportResult> {
    let (draft, states) = waiting_draft(conn, id)?;
    let check = check_draft(conn, &draft)?;
    write_import(conn, &draft, &states.lines, &check, mode, now)
}

fn write_import(
    conn: &Transaction<'_>,
    draft: &Draft,
    states: &[LineState],
    check: &DraftCheck,
    mode: ImportMode,
    now: &str,
) -> AppResult<ImportResult> {
    let chosen = selected_questions(states, check, mode)?;
    let (outcome, local_ids) = import_selected(conn, draft, states, &chosen, now)?;
    let title = collection_title(draft);
    let collection_id =
        create_collection(conn, &title, draft.request.request.board.as_deref(), now)?;
    place_questions(conn, &collection_id, &local_ids, &outcome)?;
    draft::set_status(conn, &draft.id, DraftStatus::Imported)?;
    Ok(ImportResult {
        collection_id,
        title,
        imported: count_u32(outcome.inserted),
        merged: count_u32(outcome.merged),
        skipped: count_u32(states.len() - chosen.len()),
    })
}

fn import_problems() -> AppError {
    AppError::Invalid(
        "O pacote tem problemas. Importe só as confirmadas ou descarte o pacote.".into(),
    )
}

fn selected_questions(
    states: &[LineState],
    check: &DraftCheck,
    mode: ImportMode,
) -> AppResult<Vec<usize>> {
    let total = states.len();
    let chosen: Vec<usize> = match mode {
        ImportMode::All => {
            if states.contains(&LineState::Unchecked) {
                return Err(AppError::Invalid(
                    "Há questões sem conferência. Confira-as antes de importar tudo.".into(),
                ));
            }
            if !check.problems.is_empty() {
                return Err(import_problems());
            }
            (0..total).collect()
        }
        ImportMode::Confirmed => states
            .iter()
            .enumerate()
            .filter_map(|(index, state)| {
                (matches!(state, LineState::Agreed | LineState::Fixed) && !check.is_blocked(index))
                    .then_some(index)
            })
            .collect(),
    };
    if chosen.is_empty() {
        return Err(AppError::Invalid(
            "Nenhuma questão confirmada para importar.".into(),
        ));
    }

    Ok(chosen)
}

fn import_selected(
    conn: &Transaction<'_>,
    draft: &Draft,
    states: &[LineState],
    chosen: &[usize],
    now: &str,
) -> AppResult<(ImportOutcome, Vec<String>)> {
    // Questions the blind check still disagreed with get the badge.
    let disputed: Vec<_> = states
        .iter()
        .enumerate()
        .filter_map(|(index, state)| (*state == LineState::Disputed).then_some(index))
        .collect();
    let inputs: Vec<_> = draft_inputs(draft, &disputed)
        .into_iter()
        .enumerate()
        .filter(|(index, _)| chosen.contains(index))
        .map(|(_, input)| input)
        .collect();
    let local_ids: Vec<String> = inputs.iter().map(|i| i.id.clone()).collect();
    let outcome = import_questions(conn, inputs, now).map_err(|error| match error {
        // The detailed message quotes question text; the preview must not.
        AppError::Invalid(_) => AppError::Invalid("Não foi possível importar o pacote.".into()),
        other => other,
    })?;

    Ok((outcome, local_ids))
}

fn collection_title(draft: &Draft) -> String {
    let request = &draft.request.request;
    let topic = &request.topic;
    let day = draft.created_at.get(..10).unwrap_or_default();
    let date = match day.split('-').collect::<Vec<_>>()[..] {
        [year, month, day] => format!("{day}/{month}/{year}"),
        _ => day.to_owned(),
    };
    format!("{topic} · gerado em {date}")
}

fn create_collection(
    conn: &Connection,
    title: &str,
    board: Option<&str>,
    now: &str,
) -> AppResult<CollectionId> {
    let collection_id = ids::generate::<CollectionId>(&mut rand::rng());
    conn.execute(
        "INSERT INTO collections (id, title, kind, contest, board, created_at)
         VALUES (?1, ?2, ?3, NULL, ?4, ?5)",
        params![collection_id, title, CollectionKind::Generated, board, now],
    )?;
    Ok(collection_id)
}

fn place_questions(
    conn: &Connection,
    collection_id: &CollectionId,
    local_ids: &[String],
    outcome: &ImportOutcome,
) -> AppResult<()> {
    let mut position = 0;
    let mut placed: Vec<&QuestionId> = Vec::new();
    for local_id in local_ids {
        let question_id = outcome
            .ids_by_local_id
            .get(local_id)
            .ok_or_else(|| AppError::Internal("imported question has no canonical ID".into()))?;
        if placed.contains(&question_id) {
            continue;
        }
        placed.push(question_id);
        conn.execute(
            "INSERT INTO collection_items (collection_id, position, question_id)
             VALUES (?1, ?2, ?3)",
            params![collection_id, position, question_id],
        )?;
        position += 1;
    }
    Ok(())
}

/// Discards a waiting draft and removes its attachments after the transaction commits.
pub fn discard(db: &Database, files_dir: &Path, id: &DraftId) -> AppResult<()> {
    db.transaction(|tx| mark_discarded(tx, id))?;
    draft_files::remove(files_dir, id);
    Ok(())
}

fn mark_discarded(conn: &Transaction<'_>, id: &DraftId) -> AppResult<()> {
    let draft = draft::load(conn, id)?;
    if draft.status != DraftStatus::Review {
        return Err(not_waiting());
    }
    if states(&draft)?.checking {
        return Err(checking_error());
    }
    draft::set_status(conn, id, DraftStatus::Discarded)
}

#[cfg(test)]
mod tests;
