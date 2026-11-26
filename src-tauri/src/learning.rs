//! Error notebook and spaced repetition (spec §3.5).
//!
//! **Reviews are derived, not accumulated.** `review_state` is a cache of one question's
//! history: [`refresh_question`] replays every answer to it from the finished attempts, in
//! the order the answers were given, against the *current* key. That makes the schedule
//! idempotent (refreshing twice changes nothing), immune to late events (an older attempt
//! finished later can't override a newer result) and automatically right after an answer-key
//! correction or an annulment, once the caller refreshes the affected question.
//!
//! The ladder is 1 → 3 → 7 → 14 → 30 → 60 days, as in the legacy app: the interval is the rung
//! for the current streak of full hits (the first hit is 1 day, the second 3, ... capped at
//! 60), and anything else, wrong or blank, resets the streak and schedules 1 day. Annulled
//! questions have no review.
//!
//! **The error notebook** is read straight from the data: a question whose latest answered response
//! in a finished attempt is wrong (a later blank doesn't hide it), or whose latest result is a
//! blank. Banco's `selection` module reads it through [`latest_results`].

use std::collections::HashSet;

use rusqlite::{Connection, Statement, params, params_from_iter};
use serde::Serialize;

use crate::attempts::{HIT_SQL, MISS_SQL, RESPONSE_TIME_SQL};
use crate::error::AppResult;
use crate::ids::{AttemptId, QuestionId};

/// Review intervals in days.
const LADDER: [u32; 6] = [1, 3, 7, 14, 30, 60];

/// A review is due at the instant of its scheduled timestamp.
pub(crate) const DUE_SQL: &str = "due_at <= ?1";

/// Recomputes one question's review from its whole history. Call it when an attempt finishes,
/// a key is corrected, or a question is annulled or restored.
pub fn refresh_question(conn: &Connection, question_id: &QuestionId) -> AppResult<()> {
    let history = review_history(conn, Some(question_id))?;
    if let Some(review) = history.into_iter().next() {
        save_review(&mut conn.prepare(SAVE_REVIEW_SQL)?, review)?;
    } else {
        conn.execute(
            "DELETE FROM review_state WHERE question_id = ?1",
            params![question_id],
        )?;
    }
    Ok(())
}

/// Refreshes every question of an attempt. Call it when the attempt finishes.
pub fn refresh_attempt(conn: &Connection, attempt_id: &AttemptId) -> AppResult<()> {
    let mut stmt =
        conn.prepare("SELECT DISTINCT question_id FROM responses WHERE attempt_id = ?1")?;
    let ids = stmt
        .query_map(params![attempt_id], |r| r.get::<_, QuestionId>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for id in ids {
        refresh_question(conn, &id)?;
    }
    Ok(())
}

/// Rebuilds every review from current keys and finished responses in one history query.
pub fn refresh_all(conn: &Connection) -> AppResult<()> {
    let history = review_history(conn, None)?;
    let mut write = conn.prepare(SAVE_REVIEW_SQL)?;
    for review in history {
        save_review(&mut write, review)?;
    }
    conn.execute(
        &format!(
            "DELETE FROM review_state WHERE question_id NOT IN (
                SELECT r.question_id {REVIEW_HISTORY_SOURCE_SQL}
             )"
        ),
        [],
    )?;
    Ok(())
}

pub(crate) fn due_count(conn: &Connection, now: &str) -> AppResult<u32> {
    let sql = format!("SELECT COUNT(*) FROM review_state WHERE {DUE_SQL}");
    Ok(conn.query_row(&sql, [now], |r| r.get(0))?)
}

/// How many reviews due at `now` sit in an unfinished attempt.
pub fn due_in_open_attempt(conn: &Connection, now: &str) -> AppResult<u32> {
    Ok(conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM review_state
             WHERE {DUE_SQL} AND question_id IN ({OPEN_ATTEMPT_QUESTIONS_SQL})"
        ),
        [now],
        |r| r.get(0),
    )?)
}

/// What `review_state.state` holds.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewState {
    interval_days: u32,
    /// When the answer that set this interval was given.
    last_answered_at: String,
}

struct ReviewHistory {
    question_id: QuestionId,
    last_at: String,
    hits: i64,
}

const REVIEW_HISTORY_SOURCE_SQL: &str = "FROM responses r
    JOIN attempts a ON a.id = r.attempt_id AND a.finished_at IS NOT NULL
    JOIN questions q ON q.id = r.question_id AND q.annulled = 0";

/// The newest response time and consecutive hits for each question. The first miss while
/// reading newest first ends the streak; a blank is a miss here.
fn review_history(
    conn: &Connection,
    question_id: Option<&QuestionId>,
) -> AppResult<Vec<ReviewHistory>> {
    let filter = if question_id.is_some() {
        "WHERE r.question_id = ?1"
    } else {
        ""
    };
    let mut stmt = conn.prepare(&format!(
        "WITH ranked AS (
            SELECT r.question_id, {RESPONSE_TIME_SQL} AS at, ({HIT_SQL}) AS hit,
                   ROW_NUMBER() OVER (
                       PARTITION BY r.question_id
                       ORDER BY {RESPONSE_TIME_SQL} DESC, a.id DESC, r.position DESC
                   ) AS recency
            {REVIEW_HISTORY_SOURCE_SQL}
            {filter}
         )
         SELECT question_id, MAX(CASE WHEN recency = 1 THEN at END),
                COALESCE(MIN(CASE WHEN NOT hit THEN recency END) - 1, COUNT(*))
         FROM ranked GROUP BY question_id"
    ))?;
    let rows = stmt
        .query_map(params_from_iter(question_id), |r| {
            Ok(ReviewHistory {
                question_id: r.get(0)?,
                last_at: r.get(1)?,
                hits: r.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn interval_after(hits: i64) -> u32 {
    let rung = hits.saturating_sub(1).clamp(0, LADDER.len() as i64 - 1);
    LADDER[rung as usize]
}

const SAVE_REVIEW_SQL: &str = "INSERT INTO review_state (question_id, due_at, state)
    VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ', ?2, '+' || ?3 || ' days'), ?4)
    ON CONFLICT (question_id) DO UPDATE SET due_at = excluded.due_at, state = excluded.state";

fn save_review(stmt: &mut Statement<'_>, review: ReviewHistory) -> AppResult<()> {
    let state = ReviewState {
        interval_days: interval_after(review.hits),
        last_answered_at: review.last_at,
    };
    stmt.execute(params![
        review.question_id,
        state.last_answered_at,
        state.interval_days,
        serde_json::to_string(&state)?,
    ])?;
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LatestStatus {
    Correct,
    Blank,
    Wrong,
}

/// A question's latest result in finished attempts, keeping an earlier wrong answer visible.
pub(crate) struct Latest {
    pub(crate) question_id: QuestionId,
    pub(crate) at: String,
    pub(crate) status: LatestStatus,
}

/// A later blank never hides an earlier wrong answer. In that case the timestamp and attempt
/// ID point to the wrong answer. Otherwise they point to the latest response.
pub(crate) fn latest_results(conn: &Connection) -> AppResult<Vec<Latest>> {
    let mut stmt = conn.prepare(&format!(
        "WITH base AS (
           SELECT r.question_id, a.id AS attempt_id, r.choice, ({MISS_SQL}) AS wrong, r.position,
                  {RESPONSE_TIME_SQL} AS at
           FROM responses r
           JOIN attempts a ON a.id = r.attempt_id AND a.finished_at IS NOT NULL
           JOIN questions q ON q.id = r.question_id AND q.annulled = 0
         ),
         ranked AS (
           SELECT *, ROW_NUMBER() OVER (
                    PARTITION BY question_id ORDER BY at DESC, attempt_id DESC, position DESC
                  ) AS rank
           FROM base
         ),
         answered AS (
           SELECT question_id, attempt_id, at, wrong, ROW_NUMBER() OVER (
                    PARTITION BY question_id ORDER BY at DESC, attempt_id DESC, position DESC
                  ) AS rank
           FROM base WHERE choice IS NOT NULL
         )
         SELECT l.question_id,
                CASE WHEN w.wrong = 1 THEN w.at ELSE l.at END,
                l.choice IS NULL, COALESCE(w.wrong, 0)
         FROM ranked l
         LEFT JOIN answered w ON w.question_id = l.question_id AND w.rank = 1
         WHERE l.rank = 1
         ORDER BY 2 DESC, l.question_id"
    ))?;
    let rows = stmt
        .query_map([], |r| {
            let status = match (r.get(2)?, r.get(3)?) {
                (_, true) => LatestStatus::Wrong,
                (true, false) => LatestStatus::Blank,
                (false, false) => LatestStatus::Correct,
            };
            Ok(Latest {
                question_id: r.get(0)?,
                at: r.get(1)?,
                status,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The one definition of "held by an open attempt": the questions that sit in an unfinished
/// attempt. Practice sessions leave them out: they stay due until that attempt is finished,
/// and would otherwise be picked again. Every query that needs the rule embeds this one.
pub(crate) const OPEN_ATTEMPT_QUESTIONS_SQL: &str = "SELECT DISTINCT r.question_id FROM responses r
     JOIN attempts a ON a.id = r.attempt_id AND a.finished_at IS NULL";

pub(crate) fn open_attempt_questions(conn: &Connection) -> AppResult<HashSet<QuestionId>> {
    let ids = conn
        .prepare(OPEN_ATTEMPT_QUESTIONS_SQL)?
        .query_map([], |r| r.get::<_, QuestionId>(0))?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}

pub(crate) fn fill_review_session(
    mut due: Vec<QuestionId>,
    errors: impl IntoIterator<Item = QuestionId>,
    limit: u32,
) -> Vec<QuestionId> {
    for id in errors {
        if due.len() >= limit as usize {
            break;
        }
        if !due.contains(&id) {
            due.push(id);
        }
    }
    due
}

#[cfg(test)]
mod tests;
