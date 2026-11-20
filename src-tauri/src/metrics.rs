//! Performance metrics: topic strength from the latest answer per question, weekly trend,
//! CSV export (spec §3.4, decision 19).
//!
//! Everything is derived from current question state through [`attempts::outcome`], so key
//! corrections and annulments re-score history. One query feeds the topic groups and one
//! feeds the trend; both are folded in Rust in a single pass, with no per-row JSON parsing.
//!
//! * **Topic strength** looks at *finished* attempts only, filtered by feedback mode, and
//!   keeps the latest response of each question (by attempt finish time). A question counts
//!   once per group (area, area › matter, area › matter › topic).
//! * **Weekly trend** uses *every* answer of finished attempts, grouped by ISO week (Monday
//!   to Sunday) in the caller's local time.
//! * **Contest and board** filters match questions that belong to a collection with that tag.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

use rusqlite::{Connection, named_params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::attempts::{FeedbackMode, Outcome, Score, outcome};
use crate::bank::catalog::cmp_pt;
use crate::error::AppResult;

/// Groups with fewer answers than this carry the small-sample warning.
pub const SMALL_SAMPLE: u32 = 5;
/// How many topics the "weakest" list holds.
pub const WEAKEST_LIMIT: usize = 10;

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
    pub topic_id: i64,
    pub name: String,
    pub stats: GroupStats,
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
    pub topic_id: i64,
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

/// Tags that exist on collections, for the filter dropdowns.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FilterOptions {
    pub contests: Vec<String>,
    pub boards: Vec<String>,
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
    seen: HashSet<String>,
    outcomes: Vec<Outcome>,
    answered: u32,
    available: u32,
}

impl Acc {
    /// Adds a question unless this group already has it.
    fn add(&mut self, question_id: &str, row: &Row) {
        if !self.seen.insert(question_id.to_owned()) {
            return;
        }
        if !row.annulled {
            self.available += 1;
        }
        if let Some(outcome) = row.outcome {
            self.outcomes.push(outcome);
            if matches!(outcome, Outcome::Correct | Outcome::Wrong) {
                self.answered += 1;
            }
        }
    }

    fn stats(&self) -> GroupStats {
        let score = Score::of(self.outcomes.iter().copied());
        GroupStats {
            coverage: (self.available > 0)
                .then(|| f64::from(self.answered) / f64::from(self.available)),
            small_sample: self.answered < SMALL_SAMPLE,
            score,
            answered: self.answered,
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
    match (a.0.score.achievement, b.0.score.achievement) {
        (Some(x), Some(y)) => x
            .total_cmp(&y)
            .then(b.0.score.evaluated.cmp(&a.0.score.evaluated)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
    .then_with(|| cmp_pt(a.1, b.1))
}

const FILTERS: &str = "
    AND (:contest IS NULL OR EXISTS (
        SELECT 1 FROM collection_items ci JOIN collections c ON c.id = ci.collection_id
        WHERE ci.question_id = q.id AND c.contest = :contest))
    AND (:board IS NULL OR EXISTS (
        SELECT 1 FROM collection_items ci JOIN collections c ON c.id = ci.collection_id
        WHERE ci.question_id = q.id AND c.board = :board))";

fn tag(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty())
}

fn mode(query: &PerformanceQuery) -> Option<&'static str> {
    query.feedback_mode.map(FeedbackMode::as_str)
}

// ---- Computation -----------------------------------------------------------------------------

/// The whole performance record under the given filters.
pub fn performance(conn: &Connection, query: &PerformanceQuery) -> AppResult<Performance> {
    let (overall, areas, weakest) = strength(conn, query)?;
    Ok(Performance {
        overall,
        areas,
        weakest,
        trend: trend(conn, query)?,
        filters: filter_options(conn)?,
    })
}

type Strength = (GroupStats, Vec<AreaStats>, Vec<WeakTopic>);

fn strength(conn: &Connection, query: &PerformanceQuery) -> AppResult<Strength> {
    // One row per (question, topic). `latest` keeps the newest response of each question.
    let sql = format!(
        "WITH latest AS (
             SELECT r.question_id, r.choice,
                    ROW_NUMBER() OVER (
                        PARTITION BY r.question_id
                        ORDER BY a.finished_at DESC, a.id DESC, r.position DESC) AS rn
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
        ":mode": mode(query),
        ":contest": tag(&query.contest),
        ":board": tag(&query.board),
    })?;

    let mut overall = Acc::default();
    let mut groups: BTreeMap<Vec<String>, Acc> = BTreeMap::new();
    let mut topic_ids: BTreeMap<Vec<String>, i64> = BTreeMap::new();
    while let Some(r) = rows.next()? {
        let question_id: String = r.get(0)?;
        let answer_id: String = r.get(1)?;
        let annulled: bool = r.get(2)?;
        let topic_id: i64 = r.get(3)?;
        let path = vec![
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
        ];
        let has_response: bool = r.get(7)?;
        let choice: Option<String> = r.get(8)?;
        let row = Row {
            annulled,
            outcome: has_response.then(|| outcome(choice.as_deref(), &answer_id, annulled)),
        };
        overall.add(&question_id, &row);
        for len in 1..=path.len() {
            groups
                .entry(path[..len].to_vec())
                .or_default()
                .add(&question_id, &row);
        }
        topic_ids.insert(path, topic_id);
    }

    // Assemble the tree. `BTreeMap` keys sort by prefix, so parents precede children.
    let mut areas: Vec<AreaStats> = Vec::new();
    let mut weakest: Vec<WeakTopic> = Vec::new();
    for (path, acc) in &groups {
        let stats = acc.stats();
        match path.len() {
            1 => areas.push(AreaStats {
                name: path[0].clone(),
                stats,
                matters: Vec::new(),
            }),
            2 => {
                let area = areas.last_mut().expect("areas precede their matters");
                area.matters.push(MatterStats {
                    name: path[1].clone(),
                    stats,
                    topics: Vec::new(),
                });
            }
            _ => {
                let topic_id = topic_ids[path];
                let matter = areas
                    .last_mut()
                    .and_then(|a| a.matters.last_mut())
                    .expect("matters precede their topics");
                if stats.score.evaluated > 0 {
                    weakest.push(WeakTopic {
                        topic_id,
                        area: path[0].clone(),
                        matter: path[1].clone(),
                        topic: path[2].clone(),
                        stats: stats.clone(),
                    });
                }
                matter.topics.push(TopicStats {
                    topic_id,
                    name: path[2].clone(),
                    stats,
                });
            }
        }
    }
    for area in &mut areas {
        for matter in &mut area.matters {
            matter
                .topics
                .sort_by(|a, b| weakest_first((&a.stats, &a.name), (&b.stats, &b.name)));
        }
        area.matters
            .sort_by(|a, b| weakest_first((&a.stats, &a.name), (&b.stats, &b.name)));
    }
    areas.sort_by(|a, b| weakest_first((&a.stats, &a.name), (&b.stats, &b.name)));
    weakest.sort_by(|a, b| weakest_first((&a.stats, &a.topic), (&b.stats, &b.topic)));
    weakest.truncate(WEAKEST_LIMIT);

    Ok((overall.stats(), areas, weakest))
}

/// *Acerto entre respondidas* per topic id for the Banco tree, over all finished attempts.
/// Topics without an answer are absent.
pub fn topic_accuracy(conn: &Connection) -> AppResult<HashMap<i64, f64>> {
    let (_, areas, _) = strength(conn, &PerformanceQuery::default())?;
    Ok(areas
        .iter()
        .flat_map(|a| &a.matters)
        .flat_map(|m| &m.topics)
        .filter_map(|t| Some((t.topic_id, t.stats.score.accuracy?)))
        .collect())
}

fn trend(conn: &Connection, query: &PerformanceQuery) -> AppResult<Vec<WeekPoint>> {
    // `date(ts, '-6 days', 'weekday 1')` is the Monday of the ts's week.
    let sql = format!(
        "SELECT date(r.answered_at, :offset, '-6 days', 'weekday 1'), r.choice, q.answer_id, q.annulled
         FROM responses r
         JOIN attempts a ON a.id = r.attempt_id
         JOIN questions q ON q.id = r.question_id
         WHERE a.finished_at IS NOT NULL AND r.choice IS NOT NULL AND r.answered_at IS NOT NULL
           AND (:mode IS NULL OR a.feedback_mode = :mode) {FILTERS}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(named_params! {
        ":offset": format!("{:+} minutes", query.utc_offset_minutes),
        ":mode": mode(query),
        ":contest": tag(&query.contest),
        ":board": tag(&query.board),
    })?;
    let mut weeks: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    while let Some(r) = rows.next()? {
        let week: Option<String> = r.get(0)?;
        let choice: String = r.get(1)?;
        let answer_id: String = r.get(2)?;
        let annulled: bool = r.get(3)?;
        let Some(week) = week else { continue };
        match outcome(Some(&choice), &answer_id, annulled) {
            Outcome::Correct => {
                let entry = weeks.entry(week).or_default();
                entry.0 += 1;
                entry.1 += 1;
            }
            Outcome::Wrong => weeks.entry(week).or_default().0 += 1,
            Outcome::Blank | Outcome::Annulled => {}
        }
    }
    Ok(weeks
        .into_iter()
        .map(|(week_start, (answered, correct))| WeekPoint {
            week_start,
            answered,
            correct,
            accuracy: (answered > 0).then(|| f64::from(correct) / f64::from(answered)),
        })
        .collect())
}

fn filter_options(conn: &Connection) -> AppResult<FilterOptions> {
    let column = |name: &str| -> AppResult<Vec<String>> {
        let mut values: Vec<String> = conn
            .prepare(&format!(
                "SELECT DISTINCT {name} FROM collections WHERE {name} IS NOT NULL AND trim({name}) <> ''"
            ))?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        values.sort_by(|a, b| cmp_pt(a, b));
        Ok(values)
    };
    Ok(FilterOptions {
        contests: column("contest")?,
        boards: column("board")?,
    })
}

// ---- CSV export ------------------------------------------------------------------------------

/// UTF-8 byte order mark: Excel needs it to read accents correctly.
const BOM: &str = "\u{feff}";

/// One cell. Text a spreadsheet could run as a formula (`=`, `+`, `-`, `@`, tab, CR) gets a
/// leading apostrophe; separators, quotes and line breaks are quoted.
fn cell(text: &str) -> String {
    let guarded = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    if guarded.contains([';', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

/// A ratio as a percentage with a decimal comma, or empty.
fn percent(value: Option<f64>) -> String {
    value
        .map(|v| format!("{:.1}", v * 100.0).replace('.', ","))
        .unwrap_or_default()
}

fn csv_row(level: &str, area: &str, matter: &str, topic: &str, stats: &GroupStats) -> String {
    let score = &stats.score;
    let fields = [
        cell(level),
        cell(area),
        cell(matter),
        cell(topic),
        score.correct.to_string(),
        score.wrong.to_string(),
        score.blank.to_string(),
        score.annulled.to_string(),
        score.evaluated.to_string(),
        percent(score.achievement),
        percent(score.accuracy),
        stats.available.to_string(),
        percent(stats.coverage),
    ];
    fields.join(";")
}

/// The area › matter › topic table as CSV (`;`-separated, UTF-8 with BOM, CRLF lines).
pub fn to_csv(performance: &Performance) -> String {
    let mut out = String::from(BOM);
    out.push_str(
        "Nível;Área;Matéria;Assunto;Certas;Erradas;Em branco;Anuladas;Avaliadas;\
         Aproveitamento (%);Acerto entre respondidas (%);Disponíveis;Cobertura (%)\r\n",
    );
    let mut line = |text: String| {
        out.push_str(&text);
        out.push_str("\r\n");
    };
    line(csv_row("Geral", "", "", "", &performance.overall));
    for area in &performance.areas {
        line(csv_row("Área", &area.name, "", "", &area.stats));
        for matter in &area.matters {
            line(csv_row(
                "Matéria",
                &area.name,
                &matter.name,
                "",
                &matter.stats,
            ));
            for topic in &matter.topics {
                line(csv_row(
                    "Assunto",
                    &area.name,
                    &matter.name,
                    &topic.name,
                    &topic.stats,
                ));
            }
        }
    }
    out
}

/// The CSV for the given filters.
pub fn export_csv(conn: &Connection, query: &PerformanceQuery) -> AppResult<String> {
    Ok(to_csv(&performance(conn, query)?))
}

#[cfg(test)]
mod tests;
