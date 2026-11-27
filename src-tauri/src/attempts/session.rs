use rand::{Rng, seq::SliceRandom};
use rusqlite::{Connection, OptionalExtension, params};

use super::view::collection_title;
use super::{
    AttemptId, AttemptView, CollectionId, QuestionId, QuestionOption, QuestionType, SessionSource,
    SessionStart, Snapshot, SourceKind, SourceRef, load,
};
use crate::{
    error::{AppError, AppResult},
    ids,
};

/// Starts a session with the supplied source of randomness for IDs and shuffling.
pub fn start(
    conn: &Connection,
    request: &SessionStart,
    now: &str,
    rng: &mut impl Rng,
) -> AppResult<AttemptView> {
    let (kind, source_ref, mut questions) = match &request.source {
        SessionSource::Collection { collection_id } => (
            SourceKind::Collection,
            Some(SourceRef::Collection(collection_id.clone())),
            collection_questions(conn, collection_id)?,
        ),
        SessionSource::Filter {
            filter,
            count,
            order,
        } => (
            SourceKind::Filter,
            Some(SourceRef::Filter(filter.clone())),
            crate::selection::pick(conn, filter, *count, *order, now, rng)?,
        ),
    };
    if questions.is_empty() {
        return Err(AppError::Invalid(
            "Não há questões disponíveis para esta sessão.".into(),
        ));
    }
    questions.shuffle(rng);

    let id: AttemptId = ids::generate(rng);
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
        let snapshot = serde_json::to_string(&snapshot(conn, question_id, rng)?)?;
        insert.execute(params![id, position as i64, question_id, snapshot])?;
    }
    load(conn, &id)
}

fn collection_questions(
    conn: &Connection,
    collection_id: &CollectionId,
) -> AppResult<Vec<QuestionId>> {
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
    Ok(ids)
}

/// Builds the shown snapshot of a question: options shuffled, except Certo/Errado, which
/// keeps its fixed order.
fn snapshot(
    conn: &Connection,
    question_id: &QuestionId,
    rng: &mut impl Rng,
) -> AppResult<Snapshot> {
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
        options.shuffle(rng);
    }
    Ok(Snapshot {
        kind,
        stem,
        options,
        image,
        image_alt,
    })
}
