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
//! **The error notebook** is read straight from the data: a question whose latest result in
//! a finished attempt is wrong (or blank, when asked for).

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bank::TopicPath;
use crate::error::AppResult;

/// Review intervals in days.
const LADDER: [u32; 6] = [1, 3, 7, 14, 30, 60];

/// Upcoming reviews shown in the list.
const MAX_UPCOMING_ROWS: usize = 50;

/// Largest practice session built from errors or reviews.
pub const MAX_SESSION: u32 = 100;

/// What `review_state.state` holds.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewState {
    interval_days: u32,
    /// When the answer that set this interval was given.
    last_answered_at: String,
}

/// Every answer to a question in a finished attempt, as `(timestamp, hit)`, oldest first.
/// `hit` is `attempts::outcome` written in SQL (`choice = answer_id`; annulled questions are
/// skipped by the callers), because replaying history row by row in Rust would be slower. The
/// test `scoring_in_sql_agrees_with_attempts_outcome` keeps the two together.
/// A response without an answer time (a blank) is dated by when its attempt finished.
fn history(conn: &Connection, question_id: &str) -> AppResult<Vec<(String, bool)>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(r.answered_at, a.finished_at) AS at,
                r.choice IS NOT NULL AND r.choice = q.answer_id
         FROM responses r
         JOIN attempts a ON a.id = r.attempt_id AND a.finished_at IS NOT NULL
         JOIN questions q ON q.id = r.question_id
         WHERE r.question_id = ?1
         ORDER BY at, a.id, r.position",
    )?;
    let rows = stmt
        .query_map(params![question_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Recomputes one question's review from its whole history. Call it whenever something that
/// feeds the schedule changes: an attempt finishes, a key is corrected, a question is
/// annulled or restored.
pub fn refresh_question(conn: &Connection, question_id: &str) -> AppResult<()> {
    let annulled: Option<bool> = conn
        .query_row(
            "SELECT annulled FROM questions WHERE id = ?1",
            params![question_id],
            |r| r.get(0),
        )
        .optional()?;
    let events = if annulled == Some(false) {
        history(conn, question_id)?
    } else {
        Vec::new()
    };
    let Some((last_at, _)) = events.last().cloned() else {
        conn.execute(
            "DELETE FROM review_state WHERE question_id = ?1",
            params![question_id],
        )?;
        return Ok(());
    };
    let streak = events
        .iter()
        .fold(0usize, |streak, (_, hit)| if *hit { streak + 1 } else { 0 });
    let interval = LADDER[streak.saturating_sub(1).min(LADDER.len() - 1)];
    let state = serde_json::to_string(&ReviewState {
        interval_days: interval,
        last_answered_at: last_at.clone(),
    })?;
    conn.execute(
        "INSERT INTO review_state (question_id, due_at, state)
         VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ', ?2, '+' || ?3 || ' days'), ?4)
         ON CONFLICT (question_id) DO UPDATE SET due_at = excluded.due_at, state = excluded.state",
        params![question_id, last_at, interval, state],
    )?;
    Ok(())
}

/// Refreshes every question of an attempt. Call it when the attempt finishes.
pub fn refresh_attempt(conn: &Connection, attempt_id: &str) -> AppResult<()> {
    let mut stmt =
        conn.prepare("SELECT DISTINCT question_id FROM responses WHERE attempt_id = ?1")?;
    let ids = stmt
        .query_map(params![attempt_id], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for id in ids {
        refresh_question(conn, &id)?;
    }
    Ok(())
}

/// Rebuilds every review from the data. Reviews are derived, so this is always safe; it runs
/// at startup so history that predates the feature, or a restored backup, gets its schedule.
pub fn refresh_all(conn: &Connection) -> AppResult<()> {
    let mut stmt = conn.prepare("SELECT DISTINCT question_id FROM responses")?;
    let ids = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for id in ids {
        refresh_question(conn, &id)?;
    }
    conn.execute(
        "DELETE FROM review_state WHERE question_id NOT IN (SELECT question_id FROM responses)",
        [],
    )?;
    Ok(())
}

// ---- Error notebook --------------------------------------------------------------------------

/// A question's latest result in a finished attempt.
struct Latest {
    question_id: String,
    at: String,
    attempt_id: String,
    blank: bool,
    wrong: bool,
}

/// The latest result of every non-annulled question that has one, newest first.
fn latest_results(conn: &Connection) -> AppResult<Vec<Latest>> {
    let mut stmt = conn.prepare(
        "WITH ranked AS (
           SELECT r.question_id, a.id AS attempt_id, r.choice, q.answer_id,
                  COALESCE(r.answered_at, a.finished_at) AS at,
                  ROW_NUMBER() OVER (
                    PARTITION BY r.question_id
                    ORDER BY COALESCE(r.answered_at, a.finished_at) DESC, a.id DESC, r.position DESC
                  ) AS rank
           FROM responses r
           JOIN attempts a ON a.id = r.attempt_id AND a.finished_at IS NOT NULL
           JOIN questions q ON q.id = r.question_id AND q.annulled = 0
         )
         SELECT question_id, at, attempt_id, choice IS NULL,
                choice IS NOT NULL AND choice <> answer_id
         FROM ranked WHERE rank = 1
         ORDER BY at DESC, question_id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Latest {
                question_id: r.get(0)?,
                at: r.get(1)?,
                attempt_id: r.get(2)?,
                blank: r.get(3)?,
                wrong: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

impl Latest {
    fn is_error(&self, include_blanks: bool) -> bool {
        self.wrong || (include_blanks && self.blank)
    }
}

/// Questions that sit in an unfinished attempt. Practice sessions leave them out: they stay
/// due until that attempt is finished, and would otherwise be picked again.
fn open_attempt_questions(conn: &Connection) -> AppResult<HashSet<String>> {
    let ids = conn
        .prepare(
            "SELECT DISTINCT r.question_id FROM responses r
             JOIN attempts a ON a.id = r.attempt_id AND a.finished_at IS NULL",
        )?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}

/// Questions of the error notebook for a practice session, most recent error first, without
/// the ones already in an unfinished attempt.
pub fn error_questions(
    conn: &Connection,
    include_blanks: bool,
    limit: u32,
) -> AppResult<Vec<String>> {
    let in_open_attempt = open_attempt_questions(conn)?;
    Ok(latest_results(conn)?
        .into_iter()
        .filter(|l| l.is_error(include_blanks) && !in_open_attempt.contains(&l.question_id))
        .take(limit as usize)
        .map(|l| l.question_id)
        .collect())
}

/// The questions of a reviews session: due reviews first (most overdue first), then the most
/// recent errors that aren't due yet, up to `limit`.
pub fn review_session(conn: &Connection, now: &str, limit: u32) -> AppResult<Vec<String>> {
    let mut ids = due_questions(conn, now, limit)?;
    for id in error_questions(conn, false, limit)? {
        if ids.len() >= limit as usize {
            break;
        }
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// Questions whose review is due at `now`, the most overdue first, without the ones already
/// in an unfinished attempt.
pub fn due_questions(conn: &Connection, now: &str, limit: u32) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT question_id FROM review_state
         WHERE due_at <= ?1
           AND question_id NOT IN (
             SELECT r.question_id FROM responses r
             JOIN attempts a ON a.id = r.attempt_id AND a.finished_at IS NULL)
         ORDER BY due_at, question_id LIMIT ?2",
    )?;
    let ids = stmt
        .query_map(params![now, limit], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

// ---- The page --------------------------------------------------------------------------------

/// One row of the *Erros e revisões* list. Deliberately carries no stem, option or key: it
/// links to the past attempt, where the question has already been revealed.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LearningItem {
    pub question_id: String,
    pub topics: Vec<TopicPath>,
    /// When the next review is due. `None` for an error with no review (never the case for
    /// a finished attempt, but kept honest).
    pub due_at: Option<String>,
    /// The latest result is wrong (or blank, when blanks are included).
    pub is_error: bool,
    /// The attempt holding the question's latest result.
    pub last_attempt_id: String,
    pub last_answered_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LearningOverview {
    /// Reviews due now.
    pub due: u32,
    /// Reviews scheduled for later.
    pub upcoming: u32,
    /// Questions in the error notebook.
    pub errors: u32,
    /// How many questions "practice now" would take: due reviews, then recent errors (at most
    /// [`MAX_SESSION`]).
    pub practice: u32,
    /// How many questions "practice errors" would take: the notebook without the questions in an
    /// unfinished attempt (at most [`MAX_SESSION`]).
    pub practice_errors: u32,
    /// Due reviews first (most overdue first), then upcoming ones, then errors with no review.
    pub items: Vec<LearningItem>,
}

/// Counts and list for the page at `now`.
pub fn overview(conn: &Connection, now: &str, include_blanks: bool) -> AppResult<LearningOverview> {
    let due_dates: HashMap<String, String> = conn
        .prepare("SELECT question_id, due_at FROM review_state")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut topics: HashMap<String, Vec<TopicPath>> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT qt.question_id, t.area, t.matter, t.topic
         FROM question_topics qt JOIN topics t ON t.id = qt.topic_id
         ORDER BY t.area, t.matter, t.topic",
    )?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            TopicPath {
                area: r.get(1)?,
                matter: r.get(2)?,
                topic: r.get(3)?,
            },
        ))
    })? {
        let (question_id, path) = row?;
        topics.entry(question_id).or_default().push(path);
    }

    let mut items = Vec::new();
    for latest in latest_results(conn)? {
        let due_at = due_dates.get(&latest.question_id).cloned();
        let is_error = latest.is_error(include_blanks);
        if due_at.is_none() && !is_error {
            continue;
        }
        items.push(LearningItem {
            topics: topics.remove(&latest.question_id).unwrap_or_default(),
            question_id: latest.question_id,
            due_at,
            is_error,
            last_attempt_id: latest.attempt_id,
            last_answered_at: latest.at,
        });
    }
    // Reviews by due date; errors without one go last. `latest_results` is newest first, and
    // the sort is stable.
    items.sort_by(|a, b| match (&a.due_at, &b.due_at) {
        (Some(x), Some(y)) => x.cmp(y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    // Upcoming reviews of questions that are not errors can be thousands: the list keeps the
    // soonest few (the count above stays whole).
    let mut upcoming_only = 0;
    items.retain(|i| {
        let only_upcoming = i.due_at.as_deref().is_some_and(|d| d > now) && !i.is_error;
        if only_upcoming {
            upcoming_only += 1;
        }
        !only_upcoming || upcoming_only <= MAX_UPCOMING_ROWS
    });
    let due = due_dates.values().filter(|d| d.as_str() <= now).count() as u32;
    Ok(LearningOverview {
        due,
        upcoming: due_dates.len() as u32 - due,
        errors: items.iter().filter(|i| i.is_error).count() as u32,
        practice: review_session(conn, now, MAX_SESSION)?.len() as u32,
        practice_errors: error_questions(conn, include_blanks, MAX_SESSION)?.len() as u32,
        items,
    })
}

#[cfg(test)]
mod tests;
