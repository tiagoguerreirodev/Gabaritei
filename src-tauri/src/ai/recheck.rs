//! Re-check on demand (spec §2 decision 14, §3.3): after a question is revealed, the user can
//! ask the `recheck` role whether the key is right. If it says the key is wrong, the user
//! confirms and the key is changed, with an audit row; scores read the current key, so past
//! attempts are re-scored.

use rusqlite::{Connection, params};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::ondemand::OnDemand;
use super::prompts;
use super::roles::RoleConfig;
use super::secrets::Secrets;
use super::settings::{self, Sources};
use super::task::{Task, ask};
use crate::attempts;
use crate::bank::store::load_question;
use crate::db::Database;
use crate::error::{AppError, AppResult};

const MAX_REASONING: usize = 20_000;
const MAX_SOURCES: usize = 20;
const MAX_SOURCE: usize = 2_000;

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
    /// ID of the correct option when `verdict` is `wrong`; empty otherwise.
    pub corrected_key: String,
    pub reasoning: String,
    /// Verifiable sources. Never invented.
    pub sources: Vec<String>,
}

/// A validated proposal, kept in the backend until the user confirms or dismisses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub verdict: RecheckVerdict,
    pub corrected_key: Option<String>,
    pub reasoning: String,
    pub sources: Vec<String>,
    pub by: RoleConfig,
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
        Self {
            verdict: proposal.verdict,
            corrected_key: proposal.corrected_key.clone(),
            reasoning: proposal.reasoning.clone(),
            sources: proposal.sources.clone(),
        }
    }
}

fn invalid(message: &str) -> AppError {
    AppError::Invalid(message.to_owned())
}

/// Asks the re-check role about the revealed question at `position`. Blocking.
pub fn ask_recheck(
    db: &Database,
    secrets: &Secrets,
    jobs: &OnDemand,
    attempt_id: &str,
    position: u32,
) -> AppResult<RecheckView> {
    let (settings, question) = db.with(|conn| {
        let id = attempts::revealed_question(conn, attempt_id, position)?;
        let question =
            load_question(conn, &id)?.ok_or_else(|| invalid("Questão não encontrada."))?;
        Ok((settings::load(conn, secrets)?, question))
    })?;
    let role = settings.roles.recheck.clone();
    let answer: RecheckAnswer = jobs.run(|cancel| {
        let (answer, _) = ask(
            &settings,
            secrets,
            &role,
            Task {
                schema: super::blind_check::schema_of::<RecheckAnswer>(),
                // The question holds no private material, so the web is safe to allow.
                sources: Sources {
                    remnote: false,
                    web: true,
                },
                instructions: prompts::recheck_instructions(),
                prompt: prompts::recheck_prompt(&question),
                attachments: Vec::new(),
            },
            cancel,
            &mut |_| {},
        )?;
        Ok::<RecheckAnswer, AppError>(answer)
    })?;

    if answer.reasoning.trim().is_empty() || answer.reasoning.chars().count() > MAX_REASONING {
        return Err(invalid("A IA não justificou a revisão. Tente de novo."));
    }
    if answer.sources.len() > MAX_SOURCES
        || answer
            .sources
            .iter()
            .any(|s| s.trim().is_empty() || s.chars().count() > MAX_SOURCE)
    {
        return Err(invalid("A IA devolveu fontes inválidas. Tente de novo."));
    }
    let corrected_key = match answer.verdict {
        RecheckVerdict::Wrong => {
            let key = answer.corrected_key.trim();
            if key == question.answer_id || !question.options.iter().any(|o| o.id == key) {
                return Err(invalid(
                    "A IA disse que o gabarito está errado, mas não indicou uma alternativa válida.",
                ));
            }
            Some(key.to_owned())
        }
        _ => None,
    };
    let proposal = Proposal {
        verdict: answer.verdict,
        corrected_key,
        reasoning: answer.reasoning.trim().to_owned(),
        sources: answer.sources.iter().map(|s| s.trim().to_owned()).collect(),
        by: role,
    };
    let view = RecheckView::from(&proposal);
    jobs.keep(&question.id, proposal);
    Ok(view)
}

/// Applies the proposal the AI made for the revealed question: changes the key and records it.
/// Call inside a transaction.
pub fn apply(
    conn: &Connection,
    jobs: &OnDemand,
    attempt_id: &str,
    position: u32,
    now: &str,
) -> AppResult<()> {
    let question_id = attempts::revealed_question(conn, attempt_id, position)?;
    let proposal = jobs
        .take(&question_id)
        .ok_or_else(|| invalid("Não há uma revisão esperando confirmação. Peça de novo."))?;
    let (RecheckVerdict::Wrong, Some(new_key)) = (proposal.verdict, proposal.corrected_key.clone())
    else {
        return Err(invalid("Esta revisão não propõe trocar o gabarito."));
    };
    let old_key: String = conn.query_row(
        "SELECT answer_id FROM questions WHERE id = ?1",
        params![question_id],
        |row| row.get(0),
    )?;
    conn.execute(
        "UPDATE questions SET answer_id = ?2, updated_at = ?3 WHERE id = ?1",
        params![question_id, new_key, now],
    )?;
    conn.execute(
        "INSERT INTO key_changes (question_id, old_answer_id, new_answer_id, reason, source, changed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            question_id,
            old_key,
            new_key,
            proposal.reasoning,
            format!("recheck:{}/{}", proposal.by.agent.as_str(), proposal.by.model),
            now
        ],
    )?;
    Ok(())
}

/// Drops the proposal for a question without applying it.
pub fn dismiss(
    conn: &Connection,
    jobs: &OnDemand,
    attempt_id: &str,
    position: u32,
) -> AppResult<()> {
    let question_id = attempts::revealed_question(conn, attempt_id, position)?;
    jobs.take(&question_id);
    Ok(())
}

#[cfg(test)]
mod tests;
