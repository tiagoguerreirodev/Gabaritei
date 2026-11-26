//! Performance metrics: topic strength from the latest answer per question, weekly trend,
//! CSV export (spec §3.4, decision 19).
//!
//! Everything is derived from current question state through [`attempts::outcome`], so key
//! corrections and annulments re-score history. One query feeds the topic groups and one
//! feeds the trend; both are folded in Rust in a single pass, with no per-row JSON parsing.
//!
//! * **Topic strength** looks at *finished* attempts only, filtered by feedback mode, and
//!   keeps the latest *answered* response of each question (by attempt finish time); a blank
//!   response is used only for a question that was never answered, so a blank never hides
//!   an earlier answer and coverage means "ever answered". A question counts
//!   once per group (area, area › matter, area › matter › topic).
//! * **Weekly trend** uses *every* answer of finished attempts, grouped by ISO week (Monday
//!   to Sunday) in the caller's local time.
//!   Each topic also carries its own trend, cut to its latest weeks with answers.
//! * **Contest and board** filters match questions that belong to a collection with that tag.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

use rusqlite::{Connection, named_params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ai::draft::DraftStatus;
use crate::attempts::{
    self, ActiveAttempt, FeedbackMode, Outcome, RESPONSE_TIME_SQL, Score, outcome,
};
use crate::bank::TopicPath;
pub use crate::bank::tags::FilterOptions;
use crate::error::AppResult;
use crate::ids::{QuestionId, TopicId};
use crate::ordering::{cmp_pt, some_first};

/// Groups with fewer answers than this carry the small-sample warning.
pub const SMALL_SAMPLE: u32 = 5;
/// How many topics the "weakest" list holds.
pub const WEAKEST_LIMIT: usize = 10;
/// How many of a topic's most recent weeks with answers its mini trend shows.
pub const TOPIC_TREND_WEEKS: usize = 8;

// ---- Request and result shapes ---------------------------------------------------------------

/// Filters and the viewer's time zone. Empty tags mean "no filter".
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct PerformanceQuery {
    pub contest: Option<String>,
    pub board: Option<String>,
    pub feedback_mode: Option<FeedbackMode>,
    /// Minutes to add to UTC to get the viewer's local time (−180 in Brasília). Decides which
    /// ISO week an answer belongs to.
    pub utc_offset_minutes: i32,
}

/// Strength of one group of questions.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GroupStats {
    /// Outcomes of the latest answers: achievement and accuracy live here.
    pub score: Score,
    /// Distinct questions in the group, annulled ones included.
    pub questions: u32,
    /// Questions answered (correct + wrong) under the current key.
    pub answered: u32,
    /// Questions in the group that aren't annulled.
    pub available: u32,
    /// *Cobertura*: answered ÷ available.
    pub coverage: Option<f64>,
    /// Fewer than [`SMALL_SAMPLE`] answers: don't trust the percentages yet.
    pub small_sample: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TopicStats {
    pub topic_id: TopicId,
    pub name: String,
    pub stats: GroupStats,
    /// The topic's latest [`TOPIC_TREND_WEEKS`] weeks with answers, oldest first.
    pub trend: Vec<WeekPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MatterStats {
    pub name: String,
    pub stats: GroupStats,
    pub topics: Vec<TopicStats>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AreaStats {
    pub name: String,
    pub stats: GroupStats,
    pub matters: Vec<MatterStats>,
}

/// A topic in the weakest list, with its full path.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WeakTopic {
    pub topic_id: TopicId,
    pub area: String,
    pub matter: String,
    pub topic: String,
    pub stats: GroupStats,
}

/// One ISO week of answers.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WeekPoint {
    /// The Monday of the week, `YYYY-MM-DD`, in the viewer's local time.
    pub week_start: String,
    /// Answers that count (annulled questions are left out).
    pub answered: u32,
    pub correct: u32,
    pub accuracy: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Performance {
    /// Every question once, whatever its topics.
    pub overall: GroupStats,
    /// Areas, matters and topics, weakest first.
    pub areas: Vec<AreaStats>,
    /// The topics with the lowest achievement (only topics with at least one answer).
    pub weakest: Vec<WeakTopic>,
    /// Oldest week first.
    pub trend: Vec<WeekPoint>,
    pub filters: FilterOptions,
}

// ---- Accumulation ----------------------------------------------------------------------------

#[derive(Default)]
struct Acc {
    seen: HashSet<QuestionId>,
    score: Score,
    available: u32,
}

impl Acc {
    /// Adds a question unless this group already has it.
    fn add(&mut self, question_id: &QuestionId, row: &Row) {
        if !self.seen.insert(question_id.to_owned()) {
            return;
        }
        if !row.annulled {
            self.available += 1;
        }
        if let Some(outcome) = row.outcome {
            self.score.record(outcome);
        }
    }

    fn stats(self) -> GroupStats {
        let answered = self.score.answered();
        GroupStats {
            coverage: Score::ratio(answered, self.available),
            small_sample: answered < SMALL_SAMPLE,
            score: self.score,
            questions: u32::try_from(self.seen.len()).unwrap_or(u32::MAX),
            answered,
            available: self.available,
        }
    }
}

struct Row {
    annulled: bool,
    /// `None` when the question has no finished response yet.
    outcome: Option<Outcome>,
}

/// Weakest first: lowest achievement, then more evaluated questions, then name. Groups with
/// nothing evaluated go last.
fn weakest_first(a: (&GroupStats, &str), b: (&GroupStats, &str)) -> Ordering {
    some_first(
        a.0.score.achievement.as_ref(),
        b.0.score.achievement.as_ref(),
        |x, y| {
            x.total_cmp(y)
                .then(b.0.score.evaluated.cmp(&a.0.score.evaluated))
        },
    )
    .then_with(|| cmp_pt(a.1, b.1))
}

/// Contest and board must both match **the same collection** of the question.
const FILTERS: &str = "
    AND ((:contest IS NULL AND :board IS NULL) OR EXISTS (
        SELECT 1 FROM collection_items ci JOIN collections c ON c.id = ci.collection_id
        WHERE ci.question_id = q.id
          AND (:contest IS NULL OR trim(c.contest) = :contest)
          AND (:board IS NULL OR trim(c.board) = :board)))";

const MIN_UTC_OFFSET_MINUTES: i32 = -12 * 60;
const MAX_UTC_OFFSET_MINUTES: i32 = 14 * 60;

/// Offsets outside the real range of time zones (UTC−12 to UTC+14) are clamped.
fn offset_minutes(query: &PerformanceQuery) -> i32 {
    query
        .utc_offset_minutes
        .clamp(MIN_UTC_OFFSET_MINUTES, MAX_UTC_OFFSET_MINUTES)
}

fn tag(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty())
}

// ---- Computation -----------------------------------------------------------------------------

/// The whole performance record under the given filters.
pub fn performance(conn: &Connection, query: &PerformanceQuery) -> AppResult<Performance> {
    let mut groups = StrengthGroups::default();
    visit_topic_results(conn, query, |id, topic_id, path, row| {
        groups.add(id, topic_id, path, row);
    })?;
    let overall = groups.overall.stats();
    let mut areas = build_areas(groups.areas, topic_trends(conn, query)?);
    let weakest = weakest_topics(&areas);
    sort_areas(&mut areas);
    Ok(Performance {
        overall,
        areas,
        weakest,
        trend: trend(conn, query)?,
        filters: filter_options(conn)?,
    })
}

fn visit_topic_results(
    conn: &Connection,
    query: &PerformanceQuery,
    mut visit: impl FnMut(&QuestionId, TopicId, TopicPath, &Row),
) -> AppResult<()> {
    // One row per (question, topic). `latest` keeps the newest response of each question.
    let sql = format!(
        "WITH latest AS (
             SELECT r.question_id, r.choice,
                    ROW_NUMBER() OVER (
                        PARTITION BY r.question_id
                        ORDER BY r.choice IS NULL, {RESPONSE_TIME_SQL} DESC, a.id DESC,
                                 r.position DESC) AS rn
             FROM responses r JOIN attempts a ON a.id = r.attempt_id
             WHERE a.finished_at IS NOT NULL
               AND (:mode IS NULL OR a.feedback_mode = :mode))
         SELECT q.id, q.answer_id, q.annulled, t.id, t.area, t.matter, t.topic,
                l.question_id IS NOT NULL, l.choice
         FROM questions q
         JOIN question_topics qt ON qt.question_id = q.id
         JOIN topics t ON t.id = qt.topic_id
         LEFT JOIN latest l ON l.question_id = q.id AND l.rn = 1
         WHERE 1 = 1 {FILTERS}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(named_params! {
        ":mode": query.feedback_mode,
        ":contest": tag(&query.contest),
        ":board": tag(&query.board),
    })?;

    while let Some(r) = rows.next()? {
        let question_id = r.get(0)?;
        let answer_id: String = r.get(1)?;
        let annulled: bool = r.get(2)?;
        let path = TopicPath {
            area: r.get(4)?,
            matter: r.get(5)?,
            topic: r.get(6)?,
        };
        let has_response: bool = r.get(7)?;
        let choice: Option<String> = r.get(8)?;
        let row = Row {
            annulled,
            outcome: has_response.then(|| outcome(choice.as_deref(), &answer_id, annulled.into())),
        };
        visit(&question_id, r.get(3)?, path, &row);
    }
    Ok(())
}

#[derive(Default)]
struct StrengthGroups {
    overall: Acc,
    areas: BTreeMap<String, AreaGroup>,
}

#[derive(Default)]
struct AreaGroup {
    total: Acc,
    matters: BTreeMap<String, MatterGroup>,
}

#[derive(Default)]
struct MatterGroup {
    total: Acc,
    topics: BTreeMap<String, (TopicId, Acc)>,
}

impl StrengthGroups {
    fn add(&mut self, id: &QuestionId, topic_id: TopicId, path: TopicPath, row: &Row) {
        self.overall.add(id, row);
        let area = self.areas.entry(path.area).or_default();
        area.total.add(id, row);
        let matter = area.matters.entry(path.matter).or_default();
        matter.total.add(id, row);
        let (_, topic) = matter
            .topics
            .entry(path.topic)
            .or_insert_with(|| (topic_id, Acc::default()));
        topic.add(id, row);
    }
}

/// Folds the groups into areas, giving each topic its trend from `trends` (none if absent).
fn build_areas(
    groups: BTreeMap<String, AreaGroup>,
    mut trends: HashMap<TopicId, Vec<WeekPoint>>,
) -> Vec<AreaStats> {
    groups
        .into_iter()
        .map(|(name, area)| AreaStats {
            name,
            stats: area.total.stats(),
            matters: area
                .matters
                .into_iter()
                .map(|(name, matter)| MatterStats {
                    name,
                    stats: matter.total.stats(),
                    topics: matter
                        .topics
                        .into_iter()
                        .map(|(name, (topic_id, group))| TopicStats {
                            name,
                            topic_id,
                            stats: group.stats(),
                            trend: trends.remove(&topic_id).unwrap_or_default(),
                        })
                        .collect(),
                })
                .collect(),
        })
        .collect()
}

fn weakest_topics(areas: &[AreaStats]) -> Vec<WeakTopic> {
    let mut weakest = Vec::new();
    for area in areas {
        for matter in &area.matters {
            for topic in &matter.topics {
                if topic.stats.score.evaluated > 0 {
                    weakest.push(WeakTopic {
                        topic_id: topic.topic_id,
                        area: area.name.clone(),
                        matter: matter.name.clone(),
                        topic: topic.name.clone(),
                        stats: topic.stats.clone(),
                    });
                }
            }
        }
    }
    weakest.sort_by(|a, b| weakest_first((&a.stats, &a.topic), (&b.stats, &b.topic)));
    weakest.truncate(WEAKEST_LIMIT);
    weakest
}

fn sort_areas(areas: &mut [AreaStats]) {
    for area in areas.iter_mut() {
        for matter in &mut area.matters {
            matter
                .topics
                .sort_by(|a, b| weakest_first((&a.stats, &a.name), (&b.stats, &b.name)));
        }
        area.matters
            .sort_by(|a, b| weakest_first((&a.stats, &a.name), (&b.stats, &b.name)));
    }
    areas.sort_by(|a, b| weakest_first((&a.stats, &a.name), (&b.stats, &b.name)));
}

/// Strength of every area, matter and topic for the Banco tree, over all finished attempts
/// in either mode. Unlike [`performance`], groups keep name order.
pub fn bank_strength(conn: &Connection) -> AppResult<Vec<AreaStats>> {
    let mut groups = StrengthGroups::default();
    visit_topic_results(
        conn,
        &PerformanceQuery::default(),
        |id, topic_id, path, row| {
            groups.add(id, topic_id, path, row);
        },
    )?;
    Ok(build_areas(groups.areas, HashMap::new()))
}

fn trend(conn: &Connection, query: &PerformanceQuery) -> AppResult<Vec<WeekPoint>> {
    let mut weeks: BTreeMap<String, Score> = BTreeMap::new();
    visit_weekly_answers(conn, query, WeeklyScope::Overall, |_, week, result| {
        weeks.entry(week).or_default().record(result);
    })?;
    Ok(week_points(weeks))
}

/// Each topic's weekly trend, cut to its latest [`TOPIC_TREND_WEEKS`] weeks with answers.
fn topic_trends(
    conn: &Connection,
    query: &PerformanceQuery,
) -> AppResult<HashMap<TopicId, Vec<WeekPoint>>> {
    let mut topics: HashMap<TopicId, BTreeMap<String, Score>> = HashMap::new();
    visit_weekly_answers(
        conn,
        query,
        WeeklyScope::PerTopic,
        |topic_id, week, result| {
            let Some(topic_id) = topic_id else { return };
            topics
                .entry(topic_id)
                .or_default()
                .entry(week)
                .or_default()
                .record(result);
        },
    )?;
    Ok(topics
        .into_iter()
        .map(|(topic_id, weeks)| {
            let mut points = week_points(weeks);
            points.drain(..points.len().saturating_sub(TOPIC_TREND_WEEKS));
            (topic_id, points)
        })
        .collect())
}

/// Whether weekly answers are counted once or once per topic of their question.
#[derive(Clone, Copy)]
enum WeeklyScope {
    Overall,
    PerTopic,
}

/// Visits every counted answer (correct or wrong) of finished attempts with the Monday of
/// its local week, and, for [`WeeklyScope::PerTopic`], once per topic of its question.
fn visit_weekly_answers(
    conn: &Connection,
    query: &PerformanceQuery,
    scope: WeeklyScope,
    mut visit: impl FnMut(Option<TopicId>, String, Outcome),
) -> AppResult<()> {
    let (topic_column, topic_join) = match scope {
        WeeklyScope::Overall => ("NULL", ""),
        WeeklyScope::PerTopic => (
            "qt.topic_id",
            "JOIN question_topics qt ON qt.question_id = q.id",
        ),
    };
    // `date(ts, '-6 days', 'weekday 1')` is the Monday of the ts's week.
    let sql = format!(
        "SELECT date(r.answered_at, :offset, '-6 days', 'weekday 1'), r.choice, q.answer_id,
                q.annulled, {topic_column}
         FROM responses r
         JOIN attempts a ON a.id = r.attempt_id
         JOIN questions q ON q.id = r.question_id
         {topic_join}
         WHERE a.finished_at IS NOT NULL AND r.choice IS NOT NULL AND r.answered_at IS NOT NULL
           AND (:mode IS NULL OR a.feedback_mode = :mode) {FILTERS}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(named_params! {
        ":offset": format!("{:+} minutes", offset_minutes(query)),
        ":mode": query.feedback_mode,
        ":contest": tag(&query.contest),
        ":board": tag(&query.board),
    })?;
    while let Some(r) = rows.next()? {
        let week: Option<String> = r.get(0)?;
        let choice: String = r.get(1)?;
        let answer_id: String = r.get(2)?;
        let annulled: bool = r.get(3)?;
        let Some(week) = week else { continue };
        let result = outcome(Some(&choice), &answer_id, annulled.into());
        if matches!(result, Outcome::Correct | Outcome::Wrong) {
            visit(r.get(4)?, week, result);
        }
    }
    Ok(())
}

fn week_points(weeks: BTreeMap<String, Score>) -> Vec<WeekPoint> {
    weeks
        .into_iter()
        .map(|(week_start, score)| WeekPoint {
            week_start,
            answered: score.answered(),
            correct: score.correct,
            accuracy: score.accuracy,
        })
        .collect()
}

fn filter_options(conn: &Connection) -> AppResult<FilterOptions> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT nullif(trim(contest), ''), nullif(trim(board), '') FROM collections",
    )?;
    let tags = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(FilterOptions::from_tags(tags))
}

// ---- Home ------------------------------------------------------------------------------------

/// An area's headline numbers on the home page.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AreaSummary {
    pub name: String,
    pub stats: GroupStats,
}

/// The home page: numbers and what is waiting for the user.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HomeSummary {
    /// Questions in the bank.
    pub questions: u32,
    pub overall: GroupStats,
    /// Weakest first.
    pub areas: Vec<AreaSummary>,
    /// The topic with the lowest achievement among those with something evaluated: the
    /// home page suggests training it when nothing more urgent waits.
    pub weakest_topic: Option<WeakTopic>,
    /// Unfinished attempts to resume, newest first.
    pub active_attempts: Vec<ActiveAttempt>,
    /// Spaced-repetition reviews due at `now` (counts rows of `review_state`, filled by #21).
    pub due_reviews: u32,
    /// Of the due reviews, how many sit in an unfinished attempt: they stay due, but a new
    /// reviews session leaves them out.
    pub due_in_open_attempt: u32,
    /// Generated packages waiting for review.
    pub drafts_waiting: u32,
}

/// The home summary at `now` (an RFC 3339 UTC timestamp).
pub fn home(conn: &Connection, now: &str) -> AppResult<HomeSummary> {
    let mut groups = StrengthGroups::default();
    visit_topic_results(
        conn,
        &PerformanceQuery::default(),
        |id, topic_id, path, row| {
            groups.add(id, topic_id, path, row);
        },
    )?;
    let overall = groups.overall.stats();
    let areas = build_areas(groups.areas, HashMap::new());
    let weakest_topic = weakest_topics(&areas).into_iter().next();
    let mut areas: Vec<_> = areas
        .into_iter()
        .map(|area| AreaSummary {
            name: area.name,
            stats: area.stats,
        })
        .collect();
    areas.sort_by(|a, b| weakest_first((&a.stats, &a.name), (&b.stats, &b.name)));
    let count = |sql: &str, params: &[&dyn rusqlite::ToSql]| -> AppResult<u32> {
        Ok(conn.query_row(sql, params, |r| r.get(0))?)
    };
    Ok(HomeSummary {
        questions: count("SELECT COUNT(*) FROM questions", &[])?,
        overall,
        areas,
        weakest_topic,
        active_attempts: attempts::active(conn)?,
        due_reviews: crate::learning::due_count(conn, now)?,
        due_in_open_attempt: crate::learning::due_in_open_attempt(conn, now)?,
        drafts_waiting: count(
            "SELECT COUNT(*) FROM drafts WHERE status = ?1",
            &[&DraftStatus::Review],
        )?,
    })
}

mod csv;
pub use csv::export_csv;

#[cfg(test)]
mod tests;
