use super::*;

use super::view::{collection_title, topic_title};
/// Questions of a topic for a session: never answered first, then least recently answered;
/// annulled questions skipped. Ties are broken at random.
fn topic_questions(conn: &Connection, topic_id: TopicId, count: u32) -> AppResult<Vec<QuestionId>> {
    let mut stmt = conn.prepare(
        "SELECT q.id, MAX(r.answered_at)
         FROM question_topics qt
         JOIN questions q ON q.id = qt.question_id
         LEFT JOIN responses r ON r.question_id = q.id AND r.answered_at IS NOT NULL
         WHERE qt.topic_id = ?1 AND q.annulled = 0
         GROUP BY q.id",
    )?;
    let mut rows = stmt
        .query_map(params![topic_id], |r| {
            Ok((r.get::<_, QuestionId>(0)?, r.get::<_, Option<String>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.shuffle(&mut rand::rng());
    // `None` (never answered) sorts first; the sort is stable, so ties stay shuffled.
    rows.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(rows
        .into_iter()
        .take(count as usize)
        .map(|(id, _)| id)
        .collect())
}

/// Builds the shown snapshot of a question: options shuffled, except Certo/Errado, which
/// keeps its fixed order.
fn snapshot(conn: &Connection, question_id: &QuestionId) -> AppResult<Snapshot> {
    let (kind, stem, options, image, image_alt) = conn.query_row(
        "SELECT type, stem, options, image, image_alt FROM questions WHERE id = ?1",
        params![question_id],
        |r| {
            Ok((
                r.get::<_, QuestionType>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        },
    )?;
    let mut options: Vec<QuestionOption> = serde_json::from_str(&options)?;
    if kind == QuestionType::Multiple {
        options.shuffle(&mut rand::rng());
    }
    Ok(Snapshot {
        kind,
        stem,
        options,
        image,
        image_alt,
    })
}

// ---- Operations ------------------------------------------------------------------------------

/// Starts a session and returns it. Questions and options are shuffled with the OS-seeded
/// CSPRNG behind `rand::rng()`.
pub fn start(conn: &Connection, request: &SessionStart, now: &str) -> AppResult<AttemptView> {
    let (kind, source_ref, mut questions) = match &request.source {
        SessionSource::Collection { collection_id } => {
            if collection_title(conn, collection_id)?.is_none() {
                return Err(AppError::Invalid("Caderno não encontrado.".into()));
            }
            let active: Option<AttemptId> = conn
                .query_row(
                    "SELECT id FROM attempts WHERE source_kind = ?1
                       AND source_ref = ?2 AND finished_at IS NULL",
                    params![SourceKind::Collection, collection_id],
                    |r| r.get(0),
                )
                .optional()?;
            if active.is_some() {
                return Err(AppError::Invalid(
                    "Este caderno já tem uma tentativa em andamento. Continue ou abandone essa \
                     tentativa antes de começar outra."
                        .into(),
                ));
            }
            let mut stmt = conn.prepare(
                "SELECT question_id FROM collection_items WHERE collection_id = ?1
                 ORDER BY position",
            )?;
            let ids = stmt
                .query_map(params![collection_id], |r| r.get::<_, QuestionId>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            (
                SourceKind::Collection,
                Some(SourceRef::Collection(collection_id.clone())),
                ids,
            )
        }
        SessionSource::Topic { topic_id, count } => {
            if !(1..=MAX_TOPIC_SESSION).contains(count) {
                return Err(AppError::Invalid(format!(
                    "Escolha de 1 a {MAX_TOPIC_SESSION} questões."
                )));
            }
            if topic_title(conn, *topic_id)?.is_none() {
                return Err(AppError::Invalid("Assunto não encontrado.".into()));
            }
            let ids = topic_questions(conn, *topic_id, *count)?;
            (SourceKind::Topic, Some(SourceRef::Topic(*topic_id)), ids)
        }
        SessionSource::Errors { include_blanks } => (
            SourceKind::Errors,
            None,
            crate::learning::error_questions(conn, *include_blanks, crate::learning::MAX_SESSION)?,
        ),
        SessionSource::Reviews => (
            SourceKind::Reviews,
            None,
            crate::learning::review_session(conn, now, crate::learning::MAX_SESSION)?,
        ),
    };
    if questions.is_empty() {
        return Err(AppError::Invalid(
            "Não há questões disponíveis para esta sessão.".into(),
        ));
    }
    questions.shuffle(&mut rand::rng());

    let id: AttemptId = ids::generate(&mut rand::rng());
    conn.execute(
        "INSERT INTO attempts (id, source_kind, source_ref, feedback_mode, started_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, kind, source_ref, request.feedback_mode, now],
    )?;
    let mut insert = conn.prepare(
        "INSERT INTO responses (attempt_id, position, question_id, snapshot)
         VALUES (?1, ?2, ?3, ?4)",
    )?;
    for (position, question_id) in questions.iter().enumerate() {
        let snapshot = serde_json::to_string(&snapshot(conn, question_id)?)?;
        insert.execute(params![id, position as i64, question_id, snapshot])?;
    }
    load(conn, &id)
}
