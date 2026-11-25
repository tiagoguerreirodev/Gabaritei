//! Re-check on demand (spec §2 decision 14, §3.3): after a question is revealed, the user can
//! ask the `recheck` role whether the key is right. If it says the key is wrong, the user
//! confirms and the key is changed, with an audit row; scores read the current key, so past
//! attempts are re-scored.

use rusqlite::{Connection, params};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::invalid;
use super::ondemand::{
    OnDemand, RevealedAnswer, RevealedTask, ask_about_revealed, validate_sources,
};
use super::prompts;
use super::roles::{Role, RoleConfig};
use super::runner::CancelToken;
use super::secrets::Secrets;
use super::settings::ResolvedClis;
use crate::attempts;
use crate::bank::question::Question;
use crate::db::Database;
use crate::error::AppResult;
use crate::ids::AttemptId;
use crate::learning;

const MAX_REASONING: usize = 20_000;

/// What the re-checker concluded about the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum RecheckVerdict {
    Correct,
    Wrong,
    Ambiguous,
}

/// The re-checker's structured answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[schemars(title = "Revisão de gabarito")]
pub struct RecheckAnswer {
    pub verdict: RecheckVerdict,
    /// The correct option when `verdict` is `wrong`: its letter (A–E) or Certo/Errado; empty
    /// otherwise.
    pub corrected_key: String,
    pub reasoning: String,
    /// Verifiable sources. Never invented.
    pub sources: Vec<String>,
}

/// A validated proposal, kept in the backend until the user confirms or dismisses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Proposal {
    Correct(ProposalDetails),
    Wrong {
        corrected_key: String,
        details: ProposalDetails,
        by: RoleConfig,
        /// The key the AI judged. A change in the meantime makes the proposal stale.
        based_on_key: String,
    },
    Ambiguous(ProposalDetails),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProposalDetails {
    reasoning: String,
    sources: Vec<String>,
}

/// What the Resolver shows. Only produced after the question was revealed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecheckView {
    pub verdict: RecheckVerdict,
    /// Option ID of the proposed new key, only for `wrong`.
    pub corrected_key: Option<String>,
    pub reasoning: String,
    pub sources: Vec<String>,
}

impl From<&Proposal> for RecheckView {
    fn from(proposal: &Proposal) -> Self {
        let (verdict, corrected_key, details) = match proposal {
            Proposal::Correct(details) => (RecheckVerdict::Correct, None, details),
            Proposal::Wrong {
                corrected_key,
                details,
                ..
            } => (RecheckVerdict::Wrong, Some(corrected_key.clone()), details),
            Proposal::Ambiguous(details) => (RecheckVerdict::Ambiguous, None, details),
        };
        Self {
            verdict,
            corrected_key,
            reasoning: details.reasoning.clone(),
            sources: details.sources.clone(),
        }
    }
}

/// Asks the re-check role about the revealed question at `position`. Blocking.
pub fn ask_recheck(
    db: &Database,
    secrets: &Secrets,
    clis: &ResolvedClis,
    jobs: &OnDemand,
    attempt_id: &AttemptId,
    position: u32,
    cancel: &CancelToken,
) -> AppResult<RecheckView> {
    let RevealedAnswer::<RecheckAnswer> {
        question,
        role,
        answer,
    } = ask_about_revealed(
        db,
        secrets,
        clis,
        RevealedTask {
            attempt_id,
            position,
            role: Role::Recheck,
            instructions: prompts::recheck_instructions(),
            prompt: |question: &Question| Ok(prompts::recheck_prompt(question)),
        },
        cancel,
    )?;

    if answer.reasoning.trim().is_empty() || answer.reasoning.chars().count() > MAX_REASONING {
        return Err(invalid("A IA não justificou a revisão. Tente de novo."));
    }
    let details = ProposalDetails {
        reasoning: answer.reasoning.trim().to_owned(),
        sources: validate_sources(answer.sources)
            .map_err(|_| invalid("A IA devolveu fontes inválidas. Tente de novo."))?,
    };
    let proposal = match answer.verdict {
        RecheckVerdict::Wrong => {
            let option = prompts::option_by_label(&question, &answer.corrected_key)
                .filter(|option| option.id != question.answer_id)
                .ok_or_else(|| invalid(
                    "A IA disse que o gabarito está errado, mas não indicou uma alternativa válida.",
                ))?;
            Proposal::Wrong {
                corrected_key: option.id.clone(),
                details,
                by: role,
                based_on_key: question.answer_id,
            }
        }
        RecheckVerdict::Correct => Proposal::Correct(details),
        RecheckVerdict::Ambiguous => Proposal::Ambiguous(details),
    };
    let view = RecheckView::from(&proposal);
    jobs.keep(question.id, proposal);
    Ok(view)
}

/// Applies the proposal the AI made for the revealed question: changes the key and records it.
/// Call inside a transaction.
pub fn apply(
    conn: &Connection,
    jobs: &OnDemand,
    attempt_id: &AttemptId,
    position: u32,
    now: &str,
) -> AppResult<attempts::AttemptView> {
    let question_id = attempts::revealed_question(conn, attempt_id, position)?;
    let proposal = jobs
        .take(&question_id)
        .ok_or_else(|| invalid("Não há uma revisão esperando confirmação. Peça de novo."))?;
    let Proposal::Wrong {
        corrected_key: new_key,
        details,
        by,
        based_on_key,
    } = proposal
    else {
        return Err(invalid("Esta revisão não propõe trocar o gabarito."));
    };
    let old_key: String = conn.query_row(
        "SELECT answer_id FROM questions WHERE id = ?1",
        params![question_id],
        |row| row.get(0),
    )?;
    if old_key != based_on_key {
        return Err(invalid("O gabarito mudou desde a revisão. Peça de novo."));
    }
    // A corrected key settles the dispute; `key_changes` below keeps the history.
    conn.execute(
        "UPDATE questions SET answer_id = ?2, updated_at = ?3,
                provenance = json_remove(provenance, '$.disputed')
         WHERE id = ?1",
        params![question_id, new_key, now],
    )?;
    conn.execute(
        "INSERT INTO key_changes (question_id, old_answer_id, new_answer_id, reason, source, changed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            question_id,
            old_key,
            new_key,
            details.reasoning,
            format!("recheck:{}/{}", by.agent.as_str(), by.model),
            now
        ],
    )?;
    // Reviews are derived from the current key, so they follow the change.
    learning::refresh_question(conn, &question_id)?;
    attempts::load(conn, attempt_id)
}

/// Drops the proposal for a question without applying it.
pub fn dismiss(
    conn: &Connection,
    jobs: &OnDemand,
    attempt_id: &AttemptId,
    position: u32,
) -> AppResult<()> {
    let question_id = attempts::revealed_question(conn, attempt_id, position)?;
    jobs.take(&question_id);
    Ok(())
}

#[cfg(test)]
mod tests;
