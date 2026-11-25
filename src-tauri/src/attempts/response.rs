use rusqlite::{Connection, OptionalExtension, params};

use super::{AttemptView, FeedbackMode, QuestionStatus, Snapshot, load, load_header};
use crate::error::{AppError, AppResult};
use crate::ids::{AttemptId, QuestionId};

const QUESTION_NOT_FOUND: &str = "Questão não encontrada nesta tentativa.";

/// Chooses an option, or clears the choice with `None`. In Study mode a choice reveals the
/// question and locks it. Choosing an eliminated option un-eliminates it.
pub fn answer(
    conn: &Connection,
    id: &AttemptId,
    position: u32,
    option_id: Option<&str>,
    now: &str,
) -> AppResult<AttemptView> {
    let edit = editable(conn, id, position)?;
    if edit.revealed {
        return Err(locked());
    }
    let mut eliminated = edit.eliminated.clone();
    if let Some(option) = option_id {
        check_option(&edit, option)?;
        eliminated.retain(|e| e != option);
    }
    let reveal = (edit.mode == FeedbackMode::Study && option_id.is_some()).then_some(now);
    let answered_at = option_id.map(|_| now);
    save(
        conn,
        id,
        position,
        ResponseUpdate {
            choice: option_id,
            eliminated: &eliminated,
            answered_at,
            reveal_at: reveal,
        },
    )?;
    load(conn, id)
}

/// Toggles an option's elimination. Eliminating the chosen option clears the choice.
pub fn eliminate(
    conn: &Connection,
    id: &AttemptId,
    position: u32,
    option_id: &str,
) -> AppResult<AttemptView> {
    let edit = editable(conn, id, position)?;
    if edit.revealed {
        return Err(locked());
    }
    check_option(&edit, option_id)?;
    let mut eliminated = edit.eliminated.clone();
    let mut choice = edit.choice.clone();
    if let Some(index) = eliminated.iter().position(|e| e == option_id) {
        eliminated.remove(index);
    } else {
        eliminated.push(option_id.to_owned());
        if choice.as_deref() == Some(option_id) {
            choice = None;
        }
    }
    save(
        conn,
        id,
        position,
        ResponseUpdate {
            choice: choice.as_deref(),
            eliminated: &eliminated,
            answered_at: None,
            reveal_at: None,
        },
    )?;
    load(conn, id)
}

/// Sets or clears a question's flag.
pub fn flag(
    conn: &Connection,
    id: &AttemptId,
    position: u32,
    flagged: bool,
) -> AppResult<AttemptView> {
    editable(conn, id, position)?;
    conn.execute(
        "UPDATE responses SET flagged = ?3 WHERE attempt_id = ?1 AND position = ?2",
        params![id, position, flagged],
    )?;
    bump(conn, id)?;
    load(conn, id)
}

/// Finishes the attempt, revealing every question. `revision` must match the latest one, so
/// a stale screen can't finish over changes it hasn't seen.
pub fn finish(
    conn: &Connection,
    id: &AttemptId,
    revision: u32,
    now: &str,
) -> AppResult<AttemptView> {
    let header = load_header(conn, id)?;
    if header.finished_at.is_some() {
        return Err(AppError::Invalid(
            "Esta tentativa já foi finalizada.".into(),
        ));
    }
    if header.revision != revision {
        return Err(AppError::Invalid(
            "A tentativa mudou desde que esta tela foi carregada. Recarregue e tente de novo."
                .into(),
        ));
    }
    conn.execute(
        "UPDATE responses SET revealed_at = ?2 WHERE attempt_id = ?1 AND revealed_at IS NULL",
        params![id, now],
    )?;
    conn.execute(
        "UPDATE attempts SET finished_at = ?2, revision = revision + 1 WHERE id = ?1",
        params![id, now],
    )?;
    crate::learning::refresh_attempt(conn, id)?;
    load(conn, id)
}

/// Removes a revealed question from scoring across every attempt.
pub fn annul(
    conn: &Connection,
    id: &AttemptId,
    position: u32,
    now: &str,
) -> AppResult<AttemptView> {
    update_status(conn, id, position, QuestionStatus::Annulled, now)
}

pub fn restore(
    conn: &Connection,
    id: &AttemptId,
    position: u32,
    now: &str,
) -> AppResult<AttemptView> {
    update_status(conn, id, position, QuestionStatus::Valid, now)
}

/// The question at `position`, if the user has already seen its answer in this attempt. The
/// on-demand AI features (re-check, explain) are only allowed after the reveal.
pub fn revealed_question(
    conn: &Connection,
    id: &AttemptId,
    position: u32,
) -> AppResult<QuestionId> {
    revealed_id(
        conn,
        id,
        position,
        "Só é possível usar a IA depois de ver o gabarito.",
    )
}

fn revealed_id(
    conn: &Connection,
    id: &AttemptId,
    position: u32,
    hidden_message: &str,
) -> AppResult<QuestionId> {
    load_header(conn, id)?;
    let (question_id, revealed) = conn
        .query_row(
            "SELECT question_id, revealed_at IS NOT NULL FROM responses
             WHERE attempt_id = ?1 AND position = ?2",
            params![id, position],
            |r| Ok((r.get::<_, QuestionId>(0)?, r.get::<_, bool>(1)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::Invalid(QUESTION_NOT_FOUND.into()))?;
    if !revealed {
        return Err(AppError::Invalid(hidden_message.into()));
    }
    Ok(question_id)
}

/// The unfinished attempt and one of its responses, ready to change.
struct Editable {
    mode: FeedbackMode,
    options: Vec<String>,
    choice: Option<String>,
    eliminated: Vec<String>,
    revealed: bool,
}

fn editable(conn: &Connection, id: &AttemptId, position: u32) -> AppResult<Editable> {
    let header = load_header(conn, id)?;
    if header.finished_at.is_some() {
        return Err(AppError::Invalid(
            "Esta tentativa já foi finalizada e não pode mais ser alterada.".into(),
        ));
    }
    let (snap, choice, eliminated, revealed) = conn
        .query_row(
            "SELECT snapshot, choice, eliminated, revealed_at IS NOT NULL FROM responses
             WHERE attempt_id = ?1 AND position = ?2",
            params![id, position],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, bool>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| AppError::Invalid(QUESTION_NOT_FOUND.into()))?;
    let snap: Snapshot = serde_json::from_str(&snap)?;
    Ok(Editable {
        mode: header.mode,
        options: snap.options.into_iter().map(|o| o.id).collect(),
        choice,
        eliminated: serde_json::from_str(&eliminated)?,
        revealed,
    })
}

fn locked() -> AppError {
    AppError::Invalid("A resposta desta questão já foi revelada e não pode mudar.".into())
}

fn check_option(edit: &Editable, option_id: &str) -> AppResult<()> {
    if edit.options.iter().any(|o| o == option_id) {
        Ok(())
    } else {
        Err(AppError::Invalid("Alternativa inválida.".into()))
    }
}

fn bump(conn: &Connection, id: &AttemptId) -> AppResult<()> {
    conn.execute(
        "UPDATE attempts SET revision = revision + 1 WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

struct ResponseUpdate<'a> {
    choice: Option<&'a str>,
    eliminated: &'a [String],
    answered_at: Option<&'a str>,
    reveal_at: Option<&'a str>,
}

fn save(
    conn: &Connection,
    id: &AttemptId,
    position: u32,
    update: ResponseUpdate<'_>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE responses SET choice = ?3, eliminated = ?4,
                answered_at = CASE WHEN ?3 IS NULL THEN NULL ELSE COALESCE(?5, answered_at) END,
                revealed_at = COALESCE(revealed_at, ?6)
         WHERE attempt_id = ?1 AND position = ?2",
        params![
            id,
            position,
            update.choice,
            serde_json::to_string(update.eliminated)?,
            update.answered_at,
            update.reveal_at
        ],
    )?;
    bump(conn, id)
}

fn update_status(
    conn: &Connection,
    id: &AttemptId,
    position: u32,
    status: QuestionStatus,
    now: &str,
) -> AppResult<AttemptView> {
    let question_id = revealed_id(
        conn,
        id,
        position,
        "Só é possível anular uma questão depois de ver o gabarito.",
    )?;
    conn.execute(
        "UPDATE questions SET annulled = ?2, updated_at = ?3 WHERE id = ?1",
        params![question_id, status == QuestionStatus::Annulled, now],
    )?;
    crate::learning::refresh_question(conn, &question_id)?;
    load(conn, id)
}
