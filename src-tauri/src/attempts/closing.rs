use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use ts_rs::TS;

use super::{QuestionStatus, Score, outcome};
use crate::bank::TopicPath;
use crate::error::{AppError, AppResult};
use crate::ids::AttemptId;

/// What the closing screen shows next to an attempt's own score. Counts and scores only.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AttemptClosing {
    /// The latest earlier finished attempt over the same caderno or filter, if any.
    pub previous: Option<PreviousAttempt>,
    /// Score per topic, weakest first. A question with several topics counts in each.
    pub topics: Vec<TopicScore>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PreviousAttempt {
    pub id: AttemptId,
    pub finished_at: String,
    /// Derived from the current keys and annulments, like every score.
    pub score: Score,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TopicScore {
    pub path: TopicPath,
    pub score: Score,
}

/// The comparison and topic breakdown for a finished attempt.
pub fn closing(conn: &Connection, id: &AttemptId) -> AppResult<AttemptClosing> {
    Ok(AttemptClosing {
        previous: previous_attempt(conn, id)?,
        topics: topic_scores(conn, id)?,
    })
}

fn previous_attempt(conn: &Connection, id: &AttemptId) -> AppResult<Option<PreviousAttempt>> {
    let Some(finished_at) = finished_at(conn, id)? else {
        return Err(AppError::Invalid(
            "Finalize a tentativa para ver o fechamento.".into(),
        ));
    };
    let found: Option<(AttemptId, String)> = conn
        .query_row(
            "SELECT p.id, p.finished_at FROM attempts p, attempts a
             WHERE a.id = ?1 AND p.id <> a.id
               AND p.source_kind = a.source_kind AND p.source_ref IS a.source_ref
               AND (p.finished_at < ?2 OR (p.finished_at = ?2 AND p.id < a.id))
             ORDER BY p.finished_at DESC, p.id DESC LIMIT 1",
            params![id, finished_at],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((previous_id, previous_finished_at)) = found else {
        return Ok(None);
    };
    let mut scores = super::history::scores_of(conn, &[&previous_id])?;
    Ok(Some(PreviousAttempt {
        score: scores.remove(&previous_id).unwrap_or_default().0,
        id: previous_id,
        finished_at: previous_finished_at,
    }))
}

fn finished_at(conn: &Connection, id: &AttemptId) -> AppResult<Option<String>> {
    let found: Option<Option<String>> = conn
        .query_row(
            "SELECT finished_at FROM attempts WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()?;
    found.ok_or_else(|| AppError::Invalid("Tentativa não encontrada.".into()))
}

fn topic_scores(conn: &Connection, id: &AttemptId) -> AppResult<Vec<TopicScore>> {
    let mut statement = conn.prepare(
        "SELECT t.area, t.matter, t.topic, r.choice, q.answer_id, q.annulled
         FROM responses r
         JOIN questions q ON q.id = r.question_id
         JOIN question_topics qt ON qt.question_id = q.id
         JOIN topics t ON t.id = qt.topic_id
         WHERE r.attempt_id = ?1",
    )?;
    let mut rows = statement.query(params![id])?;
    let mut by_path: BTreeMap<TopicPath, Score> = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let path = TopicPath::new(
            &row.get::<_, String>(0)?,
            &row.get::<_, String>(1)?,
            &row.get::<_, String>(2)?,
        );
        let choice: Option<String> = row.get(3)?;
        let answer_id: String = row.get(4)?;
        let annulled: bool = row.get(5)?;
        by_path.entry(path).or_default().record(outcome(
            choice.as_deref(),
            &answer_id,
            QuestionStatus::from(annulled),
        ));
    }
    let mut topics: Vec<TopicScore> = by_path
        .into_iter()
        .map(|(path, score)| TopicScore { path, score })
        .collect();
    // Stable sort: topics with equal achievement stay in path order. No evaluated question last.
    topics.sort_by(|a, b| {
        let key = |t: &TopicScore| t.score.achievement.unwrap_or(f64::INFINITY);
        key(a).total_cmp(&key(b))
    });
    Ok(topics)
}
