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
//! blank, when asked for.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, Statement, params, params_from_iter};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::attempts::{HIT_SQL, MISS_SQL, RESPONSE_TIME_SQL};
use crate::bank::TopicPath;
use crate::error::AppResult;
use crate::ids::{AttemptId, QuestionId};

/// Review intervals in days.
const LADDER: [u32; 6] = [1, 3, 7, 14, 30, 60];

/// Upcoming reviews shown in the list.
const MAX_UPCOMING_ROWS: usize = 50;

/// Largest practice session built from errors or reviews.
pub const MAX_SESSION: u32 = 100;

/// A review is due at the instant of its scheduled timestamp.
pub(crate) const DUE_SQL: &str = "due_at <= ?1";

/// Which results to include in the error notebook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "bool", into = "bool")]
pub enum ErrorFilter {
    WrongOnly,
    IncludeBlanks,
}

impl From<bool> for ErrorFilter {
    fn from(include_blanks: bool) -> Self {
        if include_blanks {
            Self::IncludeBlanks
        } else {
            Self::WrongOnly
        }
    }
}

impl From<ErrorFilter> for bool {
    fn from(filter: ErrorFilter) -> Self {
        filter == ErrorFilter::IncludeBlanks
    }
}

/// Review date as shown in the computer's local calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DueState {
    Overdue,
    Today,
    Upcoming,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScheduledReview {
    pub at: String,
    pub state: DueState,
}

/// One row of the *Erros e revisões* list. Deliberately carries no stem, option or key: it
/// links to the past attempt, where the question has already been revealed.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LearningItem {
    pub question_id: QuestionId,
    pub topics: Vec<TopicPath>,
    /// When the next review is due. `None` for an error with no review (never the case for
    /// a finished attempt, but kept honest).
    pub review: Option<ScheduledReview>,
    /// The latest answered response is wrong (or the latest result is blank, when blanks are
    /// included).
    pub is_error: bool,
    /// The attempt holding the question's latest result.
    pub last_attempt_id: AttemptId,
    pub last_answered_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LearningOverview {
    /// Reviews due now.
    pub due: u32,
    /// Of the reviews due now, how many sit in an unfinished attempt: they stay due, but a new
    /// session leaves them out.
    pub due_in_open_attempt: u32,
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
    /// The next instant at which a count or a local calendar label can change.
    pub refresh_at: Option<String>,
}

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

/// Questions of the error notebook for a practice session, most recent error first, without
/// the ones already in an unfinished attempt.
pub fn error_questions(
    conn: &Connection,
    filter: ErrorFilter,
    limit: u32,
) -> AppResult<Vec<QuestionId>> {
    let in_open_attempt = open_attempt_questions(conn)?;
    let latest = latest_results(conn)?;
    Ok(error_candidates(&latest, &in_open_attempt, filter)
        .take(limit as usize)
        .cloned()
        .collect())
}

/// The questions of a reviews session: due reviews first (most overdue first), then the most
/// recent errors that aren't already selected, up to `limit`.
pub fn review_session(conn: &Connection, now: &str, limit: u32) -> AppResult<Vec<QuestionId>> {
    let due = due_questions(conn, now, limit)?;
    let errors = error_questions(conn, ErrorFilter::WrongOnly, limit)?;
    Ok(fill_review_session(due, errors, limit))
}

/// Questions whose review is due at `now`, the most overdue first, without the ones already
/// in an unfinished attempt.
pub fn due_questions(conn: &Connection, now: &str, limit: u32) -> AppResult<Vec<QuestionId>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT question_id FROM review_state
         WHERE {DUE_SQL} AND question_id NOT IN ({OPEN_ATTEMPT_QUESTIONS_SQL})
         ORDER BY due_at, question_id LIMIT ?2"
    ))?;
    let ids = stmt
        .query_map(params![now, limit], |r| r.get::<_, QuestionId>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

/// Counts and list for the page at `now`.
pub fn overview(conn: &Connection, now: &str, filter: ErrorFilter) -> AppResult<LearningOverview> {
    let latest = latest_results(conn)?;
    let in_open_attempt = open_attempt_questions(conn)?;
    let schedule = review_schedule(conn, now, &in_open_attempt)?;
    let practice = fill_review_session(
        schedule.practice_due,
        error_candidates(&latest, &in_open_attempt, ErrorFilter::WrongOnly).cloned(),
        MAX_SESSION,
    );
    let practice_errors = error_candidates(&latest, &in_open_attempt, filter)
        .take(MAX_SESSION as usize)
        .count() as u32;
    let items = overview_items(latest, &schedule.dates, question_topics(conn)?, now, filter);
    Ok(LearningOverview {
        due: schedule.due,
        due_in_open_attempt: schedule.due_in_open_attempt,
        upcoming: schedule.dates.len() as u32 - schedule.due,
        errors: items.iter().filter(|i| i.is_error).count() as u32,
        practice: practice.len() as u32,
        practice_errors,
        items,
        refresh_at: schedule.refresh_at,
    })
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

#[derive(Clone, Copy)]
enum LatestStatus {
    Correct,
    Blank,
    Wrong,
}

/// A question's latest result in finished attempts, keeping an earlier wrong answer visible.
struct Latest {
    question_id: QuestionId,
    at: String,
    attempt_id: AttemptId,
    status: LatestStatus,
}

impl Latest {
    fn is_error(&self, filter: ErrorFilter) -> bool {
        match self.status {
            LatestStatus::Correct => false,
            LatestStatus::Blank => filter == ErrorFilter::IncludeBlanks,
            LatestStatus::Wrong => true,
        }
    }
}

/// A later blank never hides an earlier wrong answer. In that case the timestamp and attempt
/// ID point to the wrong answer. Otherwise they point to the latest response.
fn latest_results(conn: &Connection) -> AppResult<Vec<Latest>> {
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
                CASE WHEN w.wrong = 1 THEN w.attempt_id ELSE l.attempt_id END,
                l.choice IS NULL, COALESCE(w.wrong, 0)
         FROM ranked l
         LEFT JOIN answered w ON w.question_id = l.question_id AND w.rank = 1
         WHERE l.rank = 1
         ORDER BY 2 DESC, l.question_id"
    ))?;
    let rows = stmt
        .query_map([], |r| {
            let status = match (r.get(3)?, r.get(4)?) {
                (_, true) => LatestStatus::Wrong,
                (true, false) => LatestStatus::Blank,
                (false, false) => LatestStatus::Correct,
            };
            Ok(Latest {
                question_id: r.get(0)?,
                at: r.get(1)?,
                attempt_id: r.get(2)?,
                status,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The one definition of "held by an open attempt": the questions that sit in an unfinished
/// attempt. Practice sessions leave them out: they stay due until that attempt is finished,
/// and would otherwise be picked again. Every query that needs the rule embeds this one.
const OPEN_ATTEMPT_QUESTIONS_SQL: &str = "SELECT DISTINCT r.question_id FROM responses r
     JOIN attempts a ON a.id = r.attempt_id AND a.finished_at IS NULL";

fn open_attempt_questions(conn: &Connection) -> AppResult<HashSet<QuestionId>> {
    let ids = conn
        .prepare(OPEN_ATTEMPT_QUESTIONS_SQL)?
        .query_map([], |r| r.get::<_, QuestionId>(0))?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}

fn error_candidates<'a>(
    latest: &'a [Latest],
    in_open_attempt: &'a HashSet<QuestionId>,
    filter: ErrorFilter,
) -> impl Iterator<Item = &'a QuestionId> {
    latest
        .iter()
        .filter(move |result| {
            result.is_error(filter) && !in_open_attempt.contains(&result.question_id)
        })
        .map(|result| &result.question_id)
}

fn fill_review_session(
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

#[derive(Default)]
struct ReviewSchedule {
    dates: HashMap<QuestionId, ScheduledReview>,
    refresh_at: Option<String>,
    due: u32,
    due_in_open_attempt: u32,
    practice_due: Vec<QuestionId>,
}

fn review_schedule(
    conn: &Connection,
    now: &str,
    in_open_attempt: &HashSet<QuestionId>,
) -> AppResult<ReviewSchedule> {
    let mut stmt = conn.prepare(&format!(
        "SELECT question_id, due_at, {DUE_SQL},
                date(due_at, 'localtime') = date(?1, 'localtime')
         FROM review_state ORDER BY due_at, question_id"
    ))?;
    let rows = stmt.query_map([now], |r| {
        let due: bool = r.get(2)?;
        let today: bool = r.get(3)?;
        let state = match (today, due) {
            (true, _) => DueState::Today,
            (false, true) => DueState::Overdue,
            (false, false) => DueState::Upcoming,
        };
        Ok((
            r.get::<_, QuestionId>(0)?,
            ScheduledReview {
                at: r.get(1)?,
                state,
            },
            due,
        ))
    })?;
    let mut schedule = ReviewSchedule::default();
    for row in rows {
        let (id, date, is_due) = row?;
        if is_due {
            schedule.due += 1;
            if in_open_attempt.contains(&id) {
                schedule.due_in_open_attempt += 1;
            } else if schedule.practice_due.len() < MAX_SESSION as usize {
                schedule.practice_due.push(id.clone());
            }
        }
        if !is_due && schedule.refresh_at.is_none() {
            schedule.refresh_at = Some(date.at.clone());
        }
        schedule.dates.insert(id, date);
    }
    if !schedule.dates.is_empty() {
        let midnight: String = conn.query_row(
            "SELECT strftime('%Y-%m-%dT%H:%M:%fZ', ?1, 'localtime', 'start of day', '+1 day', 'utc')",
            [now], |row| row.get(0),
        )?;
        schedule.refresh_at = Some(
            schedule
                .refresh_at
                .map_or(midnight.clone(), |at| at.min(midnight)),
        );
    }
    Ok(schedule)
}

fn question_topics(conn: &Connection) -> AppResult<HashMap<QuestionId, Vec<TopicPath>>> {
    let mut topics: HashMap<QuestionId, Vec<TopicPath>> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT qt.question_id, t.area, t.matter, t.topic
         FROM question_topics qt JOIN topics t ON t.id = qt.topic_id
         ORDER BY t.area, t.matter, t.topic",
    )?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, QuestionId>(0)?,
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

    Ok(topics)
}

fn overview_items(
    latest: Vec<Latest>,
    dates: &HashMap<QuestionId, ScheduledReview>,
    mut topics: HashMap<QuestionId, Vec<TopicPath>>,
    now: &str,
    filter: ErrorFilter,
) -> Vec<LearningItem> {
    let mut items = Vec::new();
    for latest in latest {
        let review = dates.get(&latest.question_id).cloned();
        let is_error = latest.is_error(filter);
        if review.is_none() && !is_error {
            continue;
        }
        items.push(LearningItem {
            topics: topics.remove(&latest.question_id).unwrap_or_default(),
            question_id: latest.question_id,
            review,
            is_error,
            last_attempt_id: latest.attempt_id,
            last_answered_at: latest.at,
        });
    }
    // Reviews by due date; errors without one go last. `latest_results` is newest first, and
    // the sort is stable.
    items.sort_by(|a, b| {
        crate::ordering::some_first(a.review.as_ref(), b.review.as_ref(), |a, b| a.at.cmp(&b.at))
    });

    // Upcoming reviews of questions that are not errors can be thousands: the list keeps the
    // soonest few (the count above stays whole).
    let mut upcoming_only = 0;
    items.retain(|i| {
        let only_upcoming = i.review.as_ref().is_some_and(|r| r.at.as_str() > now) && !i.is_error;
        if only_upcoming {
            upcoming_only += 1;
        }
        !only_upcoming || upcoming_only <= MAX_UPCOMING_ROWS
    });
    items
}

#[cfg(test)]
mod tests;
