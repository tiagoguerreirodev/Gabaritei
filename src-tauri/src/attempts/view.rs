use rusqlite::{Connection, OptionalExtension, params};

use super::{
    AttemptView, Outcome, ResponseView, Reveal, Score, Snapshot, SourceKind, SourceRef,
    load_header, outcome,
};
use crate::error::AppResult;
use crate::ids::{AttemptId, CollectionId, QuestionId, TopicId};

/// The attempt as the frontend may see it.
pub fn load(conn: &Connection, id: &AttemptId) -> AppResult<AttemptView> {
    let header = load_header(conn, id)?;
    let mut stmt = conn.prepare(
        "SELECT r.position, r.question_id, r.snapshot, r.choice, r.eliminated, r.flagged,
                r.revealed_at IS NOT NULL, q.answer_id, q.explanation, q.refs, q.annulled,
                COALESCE(json_extract(q.provenance, '$.disputed'), 0)
         FROM responses r JOIN questions q ON q.id = r.question_id
         WHERE r.attempt_id = ?1 ORDER BY r.position",
    )?;
    let rows = stmt.query_map(params![id], ResponseRow::from_row)?;
    let mut questions = Vec::new();
    let mut outcomes = Vec::new();
    for row in rows {
        let (question, outcome) = row?.into_view()?;
        questions.push(question);
        outcomes.push(outcome);
    }
    let score = header.finished_at.is_some().then(|| Score::of(outcomes));
    Ok(AttemptView {
        id: id.to_owned(),
        title: title(conn, header.source_kind, header.source_ref.as_ref())?,
        source_kind: header.source_kind,
        source_ref: header.source_ref,
        feedback_mode: header.mode,
        started_at: header.started_at,
        finished_at: header.finished_at,
        revision: header.revision,
        questions,
        score,
    })
}

pub(super) fn collection_title(conn: &Connection, id: &CollectionId) -> AppResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT title FROM collections WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?)
}

pub(super) fn topic_title(conn: &Connection, id: TopicId) -> AppResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT area, matter, topic FROM topics WHERE id = ?1",
            params![id],
            |row| {
                Ok(crate::bank::TopicPath {
                    area: row.get(0)?,
                    matter: row.get(1)?,
                    topic: row.get(2)?,
                }
                .to_string())
            },
        )
        .optional()?)
}

pub(super) fn title(
    conn: &Connection,
    kind: SourceKind,
    source_ref: Option<&SourceRef>,
) -> AppResult<String> {
    let found = match source_ref {
        Some(SourceRef::Collection(id)) => collection_title(conn, id)?,
        Some(SourceRef::Topic(id)) => topic_title(conn, *id)?,
        None => match kind {
            SourceKind::Collection | SourceKind::Topic => None,
            SourceKind::Errors => Some("Caderno de erros".into()),
            SourceKind::Reviews => Some("Revisões".into()),
        },
    };
    Ok(found.unwrap_or_else(|| "Sessão".into()))
}

struct ResponseRow {
    position: u32,
    question_id: QuestionId,
    snapshot: String,
    choice: Option<String>,
    eliminated: String,
    flagged: bool,
    revealed: bool,
    answer_id: String,
    explanation: String,
    refs: String,
    annulled: bool,
    disputed: bool,
}

impl ResponseRow {
    fn into_view(self) -> AppResult<(ResponseView, Outcome)> {
        let snap: Snapshot = serde_json::from_str(&self.snapshot)?;
        let result = outcome(
            self.choice.as_deref(),
            &self.answer_id,
            self.annulled.into(),
        );
        // The only place the key, explanation and refs enter a view.
        let reveal = if self.revealed {
            Some(Reveal {
                answer_id: self.answer_id,
                explanation: self.explanation,
                refs: serde_json::from_str(&self.refs)?,
                annulled: self.annulled,
                outcome: result,
                disputed: self.disputed,
            })
        } else {
            None
        };
        let question = ResponseView {
            position: self.position,
            question_id: self.question_id,
            kind: snap.kind,
            stem: snap.stem,
            options: snap.options,
            image: snap.image,
            image_alt: snap.image_alt,
            choice: self.choice,
            eliminated: serde_json::from_str(&self.eliminated)?,
            flagged: self.flagged,
            reveal,
        };
        Ok((question, result))
    }

    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            position: row.get(0)?,
            question_id: row.get(1)?,
            snapshot: row.get(2)?,
            choice: row.get(3)?,
            eliminated: row.get(4)?,
            flagged: row.get(5)?,
            revealed: row.get(6)?,
            answer_id: row.get(7)?,
            explanation: row.get(8)?,
            refs: row.get(9)?,
            annulled: row.get(10)?,
            disputed: row.get(11)?,
        })
    }
}
