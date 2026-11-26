//! Combined question filter for Banco: counts with facets, and the question pick for a session
//! started from it (spec §3.3–§3.5).
//!
//! The status rules are `learning`'s: the latest-result rule, the review schedule and the
//! open-attempt rule are reused, not redefined. **No spoilers (AGENTS.md rule 1):** a
//! [`SelectionSummary`] carries counts and IDs only.

use std::collections::{HashMap, HashSet};

use rand::{Rng, seq::SliceRandom};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::attempts::MAX_SELECTION_SESSION;
use crate::error::{AppError, AppResult};
use crate::ids::{AttemptId, CollectionId, QuestionId, TopicId};
use crate::learning::{self, LatestStatus};
use crate::ordering::cmp_pt;
use crate::question_count::MIN_COUNT;

/// What Banco has checked. Empty lists and `None` mean "no restriction"; the parts combine
/// with AND, and the values inside one list with OR.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct QuestionFilter {
    pub topic_ids: Vec<TopicId>,
    pub statuses: Vec<StudyStatus>,
    pub collection_id: Option<CollectionId>,
    /// "The questions of this attempt" (Refazer).
    pub attempt_id: Option<AttemptId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum StudyStatus {
    /// Never answered in any attempt.
    Unseen,
    /// The latest result in finished attempts is wrong (a later blank doesn't hide it).
    Wrong,
    /// The latest result in finished attempts is blank.
    Blank,
    /// Has a review due now.
    Due,
    /// Flagged in the question's most recently started attempt.
    Flagged,
}

impl StudyStatus {
    const ALL: [Self; 5] = [
        Self::Unseen,
        Self::Wrong,
        Self::Blank,
        Self::Due,
        Self::Flagged,
    ];

    fn holds(self, facts: &QuestionFacts) -> bool {
        match self {
            Self::Unseen => !facts.ever_answered,
            Self::Wrong => facts.latest == Some(LatestStatus::Wrong),
            Self::Blank => facts.latest == Some(LatestStatus::Blank),
            Self::Due => facts.due_at.is_some(),
            Self::Flagged => facts.flagged,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SessionOrder {
    /// Never answered first, then least recently answered.
    UnseenFirst,
    Random,
    /// Due reviews, most overdue first, then recent errors. Needs the `Due` status.
    DueFirst,
    /// Most recent error first. Needs the `Wrong` or `Blank` status.
    RecentErrorsFirst,
}

impl SessionOrder {
    /// The orders Banco may offer when these statuses are checked.
    pub fn allowed_for(statuses: &[StudyStatus]) -> Vec<Self> {
        let has = |status| statuses.contains(&status);
        let mut orders = vec![Self::UnseenFirst, Self::Random];
        if has(StudyStatus::Due) {
            orders.push(Self::DueFirst);
        }
        if has(StudyStatus::Wrong) || has(StudyStatus::Blank) {
            orders.push(Self::RecentErrorsFirst);
        }
        orders
    }

    /// Why the order can't be used without the statuses it needs, if it has such a need.
    fn requirement(self) -> Option<&'static str> {
        match self {
            Self::UnseenFirst | Self::Random => None,
            Self::DueFirst => Some("A ordem Revisões primeiro exige o status Revisão vencida."),
            Self::RecentErrorsFirst => {
                Some("A ordem Mais recentes primeiro exige o status Erradas ou Em branco.")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AreaCount {
    pub area: String,
    pub questions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StatusCount {
    pub status: StudyStatus,
    pub questions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TopicCount {
    pub topic_id: TopicId,
    pub questions: u32,
}

/// Counts for a filter. Annulled questions never match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SelectionSummary {
    pub matching: u32,
    /// Of `matching`, the questions held by an unfinished attempt.
    pub in_open_attempt: u32,
    /// Follows the whole filter.
    pub by_area: Vec<AreaCount>,
    /// Ignores the status filter, so each status shows what checking it would give.
    pub by_status: Vec<StatusCount>,
    /// Ignores the topic filter, so each topic shows what checking it would give.
    pub by_topic: Vec<TopicCount>,
    pub allowed_orders: Vec<SessionOrder>,
}

pub fn summary(
    conn: &Connection,
    filter: &QuestionFilter,
    now: &str,
) -> AppResult<SelectionSummary> {
    let candidates = load_candidates(conn, filter, now)?;
    let matching: Vec<&Candidate> = candidates.iter().filter(|c| c.matches(filter)).collect();

    let mut by_area: HashMap<&str, u32> = HashMap::new();
    for candidate in &matching {
        for area in candidate.areas() {
            *by_area.entry(area).or_default() += 1;
        }
    }
    let mut by_area: Vec<AreaCount> = by_area
        .into_iter()
        .map(|(area, questions)| AreaCount {
            area: area.to_owned(),
            questions,
        })
        .collect();
    by_area.sort_by(|a, b| cmp_pt(&a.area, &b.area));

    let by_status = StudyStatus::ALL
        .iter()
        .map(|&status| StatusCount {
            status,
            questions: count(&candidates, |c| {
                c.in_topics(filter) && status.holds(&c.facts)
            }),
        })
        .collect();

    let mut by_topic: HashMap<TopicId, u32> = HashMap::new();
    for candidate in candidates.iter().filter(|c| c.has_status(filter)) {
        for (topic_id, _) in &candidate.topics {
            *by_topic.entry(*topic_id).or_default() += 1;
        }
    }
    let mut by_topic: Vec<TopicCount> = by_topic
        .into_iter()
        .map(|(topic_id, questions)| TopicCount {
            topic_id,
            questions,
        })
        .collect();
    by_topic.sort_by_key(|entry| entry.topic_id);

    Ok(SelectionSummary {
        matching: matching.len() as u32,
        in_open_attempt: matching.iter().filter(|c| c.held).count() as u32,
        by_area,
        by_status,
        by_topic,
        allowed_orders: SessionOrder::allowed_for(&filter.statuses),
    })
}

/// Up to `count` questions matching the filter, not held by an open attempt, in `order`.
pub(crate) fn pick(
    conn: &Connection,
    filter: &QuestionFilter,
    count: u32,
    order: SessionOrder,
    now: &str,
    rng: &mut impl Rng,
) -> AppResult<Vec<QuestionId>> {
    if !(MIN_COUNT..=MAX_SELECTION_SESSION).contains(&count) {
        return Err(AppError::Invalid(format!(
            "Escolha de {MIN_COUNT} a {MAX_SELECTION_SESSION} questões."
        )));
    }
    if !SessionOrder::allowed_for(&filter.statuses).contains(&order) {
        let message = order
            .requirement()
            .unwrap_or("Ordem não permitida para este filtro.");
        return Err(AppError::Invalid(message.into()));
    }
    let mut free: Vec<Candidate> = load_candidates(conn, filter, now)?
        .into_iter()
        .filter(|c| c.matches(filter) && !c.held)
        .collect();
    free.shuffle(rng);
    let ordered = match order {
        SessionOrder::Random => free,
        SessionOrder::UnseenFirst => unseen_first(free, |c| c.last_answered.clone()),
        SessionOrder::RecentErrorsFirst => recent_errors_first(free),
        SessionOrder::DueFirst => due_first(free),
    };
    Ok(ordered
        .into_iter()
        .take(count as usize)
        .map(|c| c.id)
        .collect())
}

/// Never answered first, then least recently answered. The sort is stable, so items that tie
/// keep the order they came in (callers shuffle first to break ties at random).
pub(crate) fn unseen_first<T>(
    mut items: Vec<T>,
    last_answered: impl Fn(&T) -> Option<String>,
) -> Vec<T> {
    items.sort_by_cached_key(|item| last_answered(item));
    items
}

fn recent_errors_first<T: std::borrow::Borrow<Candidate>>(mut free: Vec<T>) -> Vec<T> {
    free.sort_by(|a, b| b.borrow().error_at.cmp(&a.borrow().error_at));
    free
}

/// Due reviews (most overdue first), then wrong answers (most recent first, as the reviews
/// session does), then whatever else matches.
fn due_first(free: Vec<Candidate>) -> Vec<Candidate> {
    let mut due: Vec<&Candidate> = free.iter().filter(|c| c.facts.due_at.is_some()).collect();
    due.sort_by(|a, b| (&a.facts.due_at, &a.id).cmp(&(&b.facts.due_at, &b.id)));
    let due_ids = due.into_iter().map(|c| c.id.clone()).collect();
    let wrong_ids = recent_errors_first(
        free.iter()
            .filter(|c| c.facts.latest == Some(LatestStatus::Wrong))
            .collect(),
    )
    .into_iter()
    .map(|c| c.id.clone());
    let mut chosen = learning::fill_review_session(due_ids, wrong_ids, u32::MAX);
    let rest: Vec<QuestionId> = free
        .iter()
        .map(|c| c.id.clone())
        .filter(|id| !chosen.contains(id))
        .collect();
    chosen.extend(rest);
    let mut by_id: HashMap<QuestionId, Candidate> =
        free.into_iter().map(|c| (c.id.clone(), c)).collect();
    chosen.iter().filter_map(|id| by_id.remove(id)).collect()
}

fn count(candidates: &[Candidate], keep: impl Fn(&Candidate) -> bool) -> u32 {
    candidates.iter().filter(|c| keep(c)).count() as u32
}

/// What is known about a question for the status rules.
struct QuestionFacts {
    ever_answered: bool,
    latest: Option<LatestStatus>,
    /// When the review is due, if it is due now.
    due_at: Option<String>,
    flagged: bool,
}

struct Candidate {
    id: QuestionId,
    /// Every topic the question is under, with its area.
    topics: Vec<(TopicId, String)>,
    facts: QuestionFacts,
    held: bool,
    last_answered: Option<String>,
    /// When the latest error (wrong or blank) happened.
    error_at: Option<String>,
}

impl Candidate {
    fn matches(&self, filter: &QuestionFilter) -> bool {
        self.in_topics(filter) && self.has_status(filter)
    }

    fn in_topics(&self, filter: &QuestionFilter) -> bool {
        filter.topic_ids.is_empty()
            || self
                .topics
                .iter()
                .any(|(id, _)| filter.topic_ids.contains(id))
    }

    fn has_status(&self, filter: &QuestionFilter) -> bool {
        filter.statuses.is_empty() || filter.statuses.iter().any(|s| s.holds(&self.facts))
    }

    fn areas(&self) -> HashSet<&str> {
        self.topics.iter().map(|(_, area)| area.as_str()).collect()
    }
}

/// Every valid question inside the filter's collection and attempt, with its facts. A fixed
/// number of queries, whatever the size of the bank.
fn load_candidates(
    conn: &Connection,
    filter: &QuestionFilter,
    now: &str,
) -> AppResult<Vec<Candidate>> {
    let in_collection = match &filter.collection_id {
        Some(id) => Some(question_set(
            conn,
            "SELECT question_id FROM collection_items WHERE collection_id = ?1",
            params![id],
        )?),
        None => None,
    };
    let in_attempt = match &filter.attempt_id {
        Some(id) => Some(question_set(
            conn,
            "SELECT question_id FROM responses WHERE attempt_id = ?1",
            params![id],
        )?),
        None => None,
    };
    let mut topics = topics_by_question(conn)?;
    let mut latest: HashMap<QuestionId, (LatestStatus, String)> = learning::latest_results(conn)?
        .into_iter()
        .map(|l| (l.question_id, (l.status, l.at)))
        .collect();
    let answered = last_answered(conn)?;
    let mut due = due_at(conn, now)?;
    let flagged = flagged_questions(conn)?;
    let held = learning::open_attempt_questions(conn)?;

    let mut stmt = conn.prepare("SELECT id FROM questions WHERE annulled = 0 ORDER BY id")?;
    let ids = stmt
        .query_map([], |r| r.get::<_, QuestionId>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids
        .into_iter()
        .filter(|id| in_collection.as_ref().is_none_or(|set| set.contains(id)))
        .filter(|id| in_attempt.as_ref().is_none_or(|set| set.contains(id)))
        .map(|id| {
            let last_result = latest.remove(&id);
            let last_answered = answered.get(&id).cloned();
            let error_at = last_result
                .as_ref()
                .filter(|(status, _)| *status != LatestStatus::Correct)
                .map(|(_, at)| at.clone());
            Candidate {
                topics: topics.remove(&id).unwrap_or_default(),
                facts: QuestionFacts {
                    ever_answered: last_answered.is_some(),
                    latest: last_result.map(|(status, _)| status),
                    due_at: due.remove(&id),
                    flagged: flagged.contains(&id),
                },
                held: held.contains(&id),
                last_answered,
                error_at,
                id,
            }
        })
        .collect())
}

fn question_set(
    conn: &Connection,
    sql: &str,
    parameters: impl rusqlite::Params,
) -> AppResult<HashSet<QuestionId>> {
    let ids = conn
        .prepare(sql)?
        .query_map(parameters, |r| r.get::<_, QuestionId>(0))?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}

fn topics_by_question(conn: &Connection) -> AppResult<HashMap<QuestionId, Vec<(TopicId, String)>>> {
    let mut stmt = conn.prepare(
        "SELECT qt.question_id, t.id, t.area FROM question_topics qt
         JOIN topics t ON t.id = qt.topic_id",
    )?;
    let mut topics: HashMap<QuestionId, Vec<(TopicId, String)>> = HashMap::new();
    for row in stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))? {
        let (question_id, topic_id, area) = row?;
        topics
            .entry(question_id)
            .or_default()
            .push((topic_id, area));
    }
    Ok(topics)
}

/// When each question was last answered with a choice, in any attempt.
fn last_answered(conn: &Connection) -> AppResult<HashMap<QuestionId, String>> {
    let mut stmt = conn.prepare(
        "SELECT question_id, COALESCE(MAX(answered_at), '') FROM responses
         WHERE choice IS NOT NULL GROUP BY question_id",
    )?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

fn due_at(conn: &Connection, now: &str) -> AppResult<HashMap<QuestionId, String>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT question_id, due_at FROM review_state WHERE {}",
        learning::DUE_SQL
    ))?;
    let rows = stmt
        .query_map(params![now], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Questions flagged in their most recently started attempt.
fn flagged_questions(conn: &Connection) -> AppResult<HashSet<QuestionId>> {
    question_set(
        conn,
        "SELECT question_id FROM (
           SELECT r.question_id, r.flagged, ROW_NUMBER() OVER (
                    PARTITION BY r.question_id ORDER BY a.started_at DESC, a.id DESC
                  ) AS rank
           FROM responses r JOIN attempts a ON a.id = r.attempt_id
         ) WHERE rank = 1 AND flagged = 1",
        [],
    )
}

#[cfg(test)]
mod tests;
