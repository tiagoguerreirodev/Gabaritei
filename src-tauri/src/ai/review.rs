//! The spoiler-free view of a draft and what the user can do with it (spec §2 decisions 10–11,
//! §3.2 "Preview").
//!
//! [`DraftSummary`] is the only draft type the frontend gets. It carries counts, validation
//! problems and one state per question: no stems, options, keys, explanations or checker
//! reasons.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::agent::{Agent, Effort};
use super::blind_check::{self, VerdictState};
use super::draft::{self, Draft, DraftStatus};
use super::generate::{DraftProblem, check_draft, draft_inputs};
use crate::bank::store::import_questions;
use crate::bank::{QuestionType, TopicPath};
use crate::error::{AppError, AppResult};

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
    pub id: String,
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
    pub id: String,
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
    pub collection_id: String,
    pub title: String,
    /// New questions added to the bank.
    pub imported: u32,
    /// Questions that were already in the bank and are now in the new collection.
    pub merged: u32,
    /// Questions left out of the import.
    pub skipped: u32,
}

fn states(draft: &Draft) -> AppResult<(Vec<LineState>, Option<String>, bool)> {
    let check = blind_check::stored(draft)?;
    let failed = check.as_ref().and_then(|c| c.failed.clone());
    let pending = check.as_ref().is_some_and(|c| c.pending);
    let lines = draft
        .package
        .questions
        .iter()
        .map(|question| {
            check
                .as_ref()
                .and_then(|c| c.verdicts.iter().find(|v| v.question_id == question.id))
                .map_or(LineState::Unchecked, |verdict| match verdict.state {
                    VerdictState::Agreed => LineState::Agreed,
                    VerdictState::Fixed => LineState::Fixed,
                    VerdictState::Disputed => LineState::Disputed,
                })
        })
        .collect();
    Ok((lines, failed, pending))
}

fn count(items: &[LineState], wanted: LineState) -> u32 {
    u32::try_from(items.iter().filter(|s| **s == wanted).count()).unwrap_or(u32::MAX)
}

pub fn summary(conn: &Connection, id: &str) -> AppResult<DraftSummary> {
    let draft = draft::load(conn, id)?;
    let check = check_draft(conn, &draft)?;
    let (states, check_failed, checking) = states(&draft)?;
    let request = &draft.request;
    Ok(DraftSummary {
        id: draft.id.clone(),
        status: draft.status,
        created_at: draft.created_at.clone(),
        topic: request.request.topic.clone(),
        count: u32::try_from(draft.package.questions.len()).unwrap_or(u32::MAX),
        multiple: u32::try_from(check.multiple).unwrap_or(u32::MAX),
        boolean: u32::try_from(check.boolean).unwrap_or(u32::MAX),
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
        lines: states
            .iter()
            .enumerate()
            .map(|(index, state)| QuestionLine {
                position: u32::try_from(index + 1).unwrap_or(u32::MAX),
                state: *state,
            })
            .collect(),
        problems: check.problems,
        duplicates: u32::try_from(check.duplicates).unwrap_or(u32::MAX),
        agreed: count(&states, LineState::Agreed),
        fixed: count(&states, LineState::Fixed),
        disputed: count(&states, LineState::Disputed),
        unchecked: count(&states, LineState::Unchecked),
        check_failed,
        checking,
    })
}

/// Drafts waiting for review, newest first.
pub fn waiting(conn: &Connection) -> AppResult<Vec<DraftListItem>> {
    let ids: Vec<String> = conn
        .prepare("SELECT id FROM drafts WHERE status = 'review' ORDER BY created_at DESC, id")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    ids.iter()
        .map(|id| {
            let draft = draft::load(conn, id)?;
            let (states, _, checking) = states(&draft)?;
            Ok(DraftListItem {
                id: draft.id.clone(),
                topic: draft.request.request.topic.clone(),
                count: u32::try_from(draft.package.questions.len()).unwrap_or(u32::MAX),
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

/// Imports a draft's questions into the bank as a new `generated` collection. Call inside a
/// transaction. Nothing is written if it fails.
pub fn import(conn: &Connection, id: &str, mode: ImportMode, now: &str) -> AppResult<ImportResult> {
    let draft = draft::load(conn, id)?;
    if draft.status != DraftStatus::Review {
        return Err(not_waiting());
    }
    let check = check_draft(conn, &draft)?;
    let (states, _, checking) = states(&draft)?;
    if checking {
        return Err(checking_error());
    }
    let total = draft.package.questions.len();
    let chosen: Vec<usize> = match mode {
        ImportMode::All => {
            if states.contains(&LineState::Unchecked) {
                return Err(AppError::Invalid(
                    "Há questões sem conferência. Confira-as antes de importar tudo.".into(),
                ));
            }
            if !check.problems.is_empty() {
                return Err(AppError::Invalid(
                    "O pacote tem problemas. Importe só as confirmadas ou descarte o pacote."
                        .into(),
                ));
            }
            (0..total).collect()
        }
        ImportMode::Confirmed => (0..total)
            .filter(|index| {
                matches!(states[*index], LineState::Agreed | LineState::Fixed)
                    && !check.is_blocked(*index)
            })
            .collect(),
    };
    if chosen.is_empty() {
        return Err(AppError::Invalid(
            "Nenhuma questão confirmada para importar.".into(),
        ));
    }

    // Questions the blind check still disagreed with get the badge.
    let inputs: Vec<_> = draft_inputs(&draft, &|index| states[index] == LineState::Disputed)
        .into_iter()
        .enumerate()
        .filter(|(index, _)| chosen.contains(index))
        .map(|(_, input)| input)
        .collect();
    let local_ids: Vec<String> = inputs.iter().map(|i| i.local_id.clone()).collect();
    let outcome = import_questions(conn, inputs, now).map_err(|error| match error {
        // The detailed message quotes question text; the preview must not.
        AppError::Invalid(_) => AppError::Invalid("Não foi possível importar o pacote.".into()),
        other => other,
    })?;

    let request = &draft.request.request;
    let topic = &request.topic;
    let day = draft.created_at.get(..10).unwrap_or_default();
    let date = match day.split('-').collect::<Vec<_>>()[..] {
        [year, month, day] => format!("{day}/{month}/{year}"),
        _ => day.to_owned(),
    };
    let title = format!(
        "{} › {} › {} · gerado em {date}",
        topic.area, topic.matter, topic.topic
    );
    let collection_id = draft::new_draft_id();
    conn.execute(
        "INSERT INTO collections (id, title, kind, contest, board, created_at)
         VALUES (?1, ?2, 'generated', NULL, ?3, ?4)",
        params![collection_id, title, request.board, now],
    )?;
    let mut position = 0;
    let mut placed: Vec<&String> = Vec::new();
    for local_id in &local_ids {
        let question_id = &outcome.ids_by_local_id[local_id];
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
    draft::set_status(conn, id, DraftStatus::Imported)?;
    Ok(ImportResult {
        collection_id,
        title,
        imported: u32::try_from(outcome.inserted).unwrap_or(u32::MAX),
        merged: u32::try_from(outcome.merged).unwrap_or(u32::MAX),
        skipped: u32::try_from(total - chosen.len()).unwrap_or(u32::MAX),
    })
}

/// Discards a draft that is still waiting.
pub fn discard(conn: &Connection, id: &str) -> AppResult<()> {
    let draft = draft::load(conn, id)?;
    if draft.status != DraftStatus::Review {
        return Err(not_waiting());
    }
    if states(&draft)?.2 {
        return Err(checking_error());
    }
    draft::set_status(conn, id, DraftStatus::Discarded)
}

#[cfg(test)]
mod tests;
