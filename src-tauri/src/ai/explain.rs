//! Explain on demand (spec §2 decision 15): fills in a missing explanation, one question at a
//! time, after the question was revealed.

use rusqlite::params;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::invalid;
use super::ondemand::{
    RevealedAnswer, RevealedTask, SourceError, ask_about_revealed, validate_sources,
};
use super::prompts;
use super::roles::Role;
use super::runner::CancelToken;
use super::secrets::Secrets;
use super::settings::ResolvedClis;
use crate::attempts::{self, AttemptView};
use crate::bank::question::{MAX_EXPLANATION, MAX_REFS, Question};
use crate::db::Database;
use crate::error::AppResult;
use crate::ids::AttemptId;

/// The explainer's structured answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Explicação")]
pub struct ExplainAnswer {
    pub explanation: String,
    /// Verifiable sources. Never invented.
    pub refs: Vec<String>,
}

/// Writes the explanation for the revealed question at `position` and returns the attempt as the
/// frontend may see it. Blocking. Refused if the question isn't revealed or already has one.
pub fn ask_explain(
    db: &Database,
    secrets: &Secrets,
    clis: &ResolvedClis,
    attempt_id: &AttemptId,
    position: u32,
    cancel: &CancelToken,
    now: impl FnOnce() -> String,
) -> AppResult<AttemptView> {
    let RevealedAnswer::<ExplainAnswer> {
        question, answer, ..
    } = ask_about_revealed(
        db,
        secrets,
        clis,
        RevealedTask {
            attempt_id,
            position,
            role: Role::Explain,
            instructions: prompts::explain_instructions(),
            prompt: |question: &Question| {
                if !question.explanation.trim().is_empty() {
                    return Err(invalid("Esta questão já tem explicação."));
                }
                Ok(prompts::explain_prompt(question))
            },
        },
        cancel,
    )?;

    let explanation = answer.explanation.trim().to_owned();
    if explanation.is_empty() || explanation.chars().count() > MAX_EXPLANATION {
        return Err(invalid(
            "A IA devolveu uma explicação inválida. Tente de novo.",
        ));
    }
    let too_many_refs = || invalid("A IA devolveu fontes demais. Tente de novo.");
    let new_refs = validate_sources(answer.refs).map_err(|error| match error {
        SourceError::TooMany => too_many_refs(),
        SourceError::InvalidReference => invalid("A IA devolveu fontes inválidas. Tente de novo."),
    })?;
    let mut refs = question.refs;
    for reference in new_refs {
        if !refs.contains(&reference) {
            refs.push(reference);
        }
    }
    if refs.len() > MAX_REFS {
        return Err(too_many_refs());
    }

    db.transaction(|tx| {
        // Only write if the explanation is still what we saw (blank): it may have changed
        // while the AI worked. Comparing the exact value avoids SQLite's `trim`, which only
        // strips spaces.
        let changed = tx.execute(
            "UPDATE questions SET explanation = ?2, refs = ?3, updated_at = ?4
             WHERE id = ?1 AND explanation = ?5",
            params![
                question.id,
                explanation,
                serde_json::to_string(&refs)?,
                now(),
                question.explanation
            ],
        )?;
        if changed == 0 {
            return Err(invalid("Esta questão já tem explicação."));
        }
        attempts::load(tx, attempt_id)
    })
}

#[cfg(test)]
mod tests;
