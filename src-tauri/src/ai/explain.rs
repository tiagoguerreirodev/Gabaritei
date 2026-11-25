//! Explain on demand (spec §2 decision 15): fills in a missing explanation, one question at a
//! time, after the question was revealed.

use rusqlite::params;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ondemand::OnDemand;
use super::prompts;
use super::secrets::Secrets;
use super::settings::{self, ResolvedClis, Sources};
use super::task::{Task, ask};
use crate::attempts::{self, AttemptView};
use crate::bank::question::{MAX_EXPLANATION, MAX_REF, MAX_REFS};
use crate::bank::store::load_question;
use crate::db::Database;
use crate::error::{AppError, AppResult};
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

fn invalid(message: &str) -> AppError {
    AppError::Invalid(message.to_owned())
}

/// Writes the explanation for the revealed question at `position` and returns the attempt as the
/// frontend may see it. Blocking. Refused if the question isn't revealed or already has one.
pub fn ask_explain(
    db: &Database,
    secrets: &Secrets,
    clis: &ResolvedClis,
    jobs: &OnDemand,
    attempt_id: &AttemptId,
    position: u32,
    now: impl FnOnce() -> String,
) -> AppResult<AttemptView> {
    let (settings, question) = db.with(|conn| {
        let id = attempts::revealed_question(conn, attempt_id, position)?;
        let question =
            load_question(conn, &id)?.ok_or_else(|| invalid("Questão não encontrada."))?;
        Ok((settings::load(conn, secrets)?, question))
    })?;
    if !question.explanation.trim().is_empty() {
        return Err(invalid("Esta questão já tem explicação."));
    }
    let role = settings.roles.explain.clone();
    let answer: ExplainAnswer = jobs.run(|cancel| {
        let (answer, _) = ask(
            &settings,
            secrets,
            clis,
            &role,
            Task {
                schema: super::blind_check::schema_of::<ExplainAnswer>(),
                sources: Sources {
                    remnote: false,
                    web: true,
                },
                instructions: prompts::explain_instructions(),
                prompt: prompts::explain_prompt(&question),
                attachments: Vec::new(),
            },
            cancel,
            &mut |_| {},
        )?;
        Ok::<ExplainAnswer, AppError>(answer)
    })?;

    let explanation = answer.explanation.trim().to_owned();
    if explanation.is_empty() || explanation.chars().count() > MAX_EXPLANATION {
        return Err(invalid(
            "A IA devolveu uma explicação inválida. Tente de novo.",
        ));
    }
    let mut refs = question.refs.clone();
    for reference in answer.refs.iter().map(|r| r.trim()) {
        if reference.is_empty() || reference.chars().count() > MAX_REF {
            return Err(invalid("A IA devolveu fontes inválidas. Tente de novo."));
        }
        if !refs.iter().any(|r| r == reference) {
            refs.push(reference.to_owned());
        }
    }
    if refs.len() > MAX_REFS {
        return Err(invalid("A IA devolveu fontes demais. Tente de novo."));
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
