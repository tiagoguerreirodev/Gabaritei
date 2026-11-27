use std::collections::HashMap;

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{FeedbackMode, QuestionStatus, Score, SourceKind, SourceRef, outcome, view::title};
use crate::bank::catalog::{DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE};
use crate::error::{AppError, AppResult};
use crate::ids::{AttemptId, CollectionId};

/// Which attempts Histórico lists, by where their questions came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum HistorySource {
    Collection,
    Filter,
    /// Topic, errors and reviews sessions: kinds that can no longer be started.
    Older,
}

impl HistorySource {
    fn kinds(self) -> &'static [SourceKind] {
        match self {
            Self::Collection => &[SourceKind::Collection],
            Self::Filter => &[SourceKind::Filter],
            Self::Older => &[SourceKind::Topic, SourceKind::Errors, SourceKind::Reviews],
        }
    }
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HistoryQuery {
    /// `None` lists every source.
    pub source: Option<HistorySource>,
    /// `None` lists both modes.
    pub mode: Option<FeedbackMode>,
    pub page: u32,
    /// 0 means the default; more than 100 is refused.
    pub page_size: u32,
}

/// A finished attempt. Carries counts and a score only, never questions or keys.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HistoryItem {
    pub id: AttemptId,
    pub title: String,
    pub source_kind: SourceKind,
    /// Set for collection attempts, so *Refazer* can open that caderno in Banco.
    pub collection_id: Option<CollectionId>,
    pub feedback_mode: FeedbackMode,
    pub started_at: String,
    pub finished_at: String,
    pub total: u32,
    /// Derived from the current keys and annulments, never stored.
    pub score: Score,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HistoryPage {
    pub items: Vec<HistoryItem>,
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
}

/// One page of finished attempts, newest first.
pub fn history(conn: &Connection, query: &HistoryQuery) -> AppResult<HistoryPage> {
    let page_size = match query.page_size {
        0 => DEFAULT_PAGE_SIZE,
        n if n > MAX_PAGE_SIZE => {
            return Err(AppError::Invalid(format!(
                "Escolha até {MAX_PAGE_SIZE} itens por página."
            )));
        }
        n => n,
    };
    let kinds = source_kinds(query.source);
    let rows = page_rows(conn, query, &kinds, page_size)?;
    let ids: Vec<&AttemptId> = rows.iter().map(|row| &row.id).collect();
    let mut scores = scores_of(conn, &ids)?;
    let items = rows
        .into_iter()
        .map(|row| {
            let source_ref = SourceRef::from_storage(row.source_kind, row.stored_ref);
            let collection_id = match &source_ref {
                Some(SourceRef::Collection(id)) => Some(id.clone()),
                _ => None,
            };
            let (score, total) = scores.remove(&row.id).unwrap_or_default();
            Ok(HistoryItem {
                title: title(conn, row.source_kind, source_ref.as_ref())?,
                id: row.id,
                source_kind: row.source_kind,
                collection_id,
                feedback_mode: row.mode,
                started_at: row.started_at,
                finished_at: row.finished_at,
                total,
                score,
            })
        })
        .collect::<AppResult<_>>()?;
    Ok(HistoryPage {
        items,
        total: count_finished(conn, query, &kinds)?,
        page: query.page,
        page_size,
    })
}

fn source_kinds(source: Option<HistorySource>) -> Vec<SourceKind> {
    match source {
        Some(source) => source.kinds().to_vec(),
        None => [
            HistorySource::Collection,
            HistorySource::Filter,
            HistorySource::Older,
        ]
        .into_iter()
        .flat_map(|source| source.kinds().iter().copied())
        .collect(),
    }
}

/// The kinds as a JSON array of stored names, for `json_each` in SQL.
fn kinds_json(kinds: &[SourceKind]) -> String {
    let names: Vec<&str> = kinds.iter().map(|kind| kind.as_str()).collect();
    serde_json::to_string(&names).expect("a list of strings serializes")
}

const FINISHED_WHERE: &str = "a.finished_at IS NOT NULL
     AND a.source_kind IN (SELECT value FROM json_each(?1))
     AND (?2 IS NULL OR a.feedback_mode = ?2)";

fn count_finished(conn: &Connection, query: &HistoryQuery, kinds: &[SourceKind]) -> AppResult<u32> {
    let sql = format!("SELECT COUNT(*) FROM attempts a WHERE {FINISHED_WHERE}");
    Ok(conn.query_row(&sql, params![kinds_json(kinds), query.mode], |r| r.get(0))?)
}

struct HistoryRow {
    id: AttemptId,
    source_kind: SourceKind,
    stored_ref: Option<String>,
    mode: FeedbackMode,
    started_at: String,
    finished_at: String,
}

/// Newest first; the ID breaks ties so a row never repeats across pages.
fn page_rows(
    conn: &Connection,
    query: &HistoryQuery,
    kinds: &[SourceKind],
    page_size: u32,
) -> AppResult<Vec<HistoryRow>> {
    let sql = format!(
        "SELECT a.id, a.source_kind, a.source_ref, a.feedback_mode, a.started_at, a.finished_at
         FROM attempts a WHERE {FINISHED_WHERE}
         ORDER BY a.finished_at DESC, a.id LIMIT ?3 OFFSET ?4"
    );
    let offset = i64::from(query.page) * i64::from(page_size);
    let rows = conn
        .prepare(&sql)?
        .query_map(
            params![kinds_json(kinds), query.mode, page_size, offset],
            |row| {
                Ok(HistoryRow {
                    id: row.get(0)?,
                    source_kind: row.get(1)?,
                    stored_ref: row.get(2)?,
                    mode: row.get(3)?,
                    started_at: row.get(4)?,
                    finished_at: row.get(5)?,
                })
            },
        )?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Each attempt's score and question count, from one query over the current question state.
pub(super) fn scores_of(
    conn: &Connection,
    ids: &[&AttemptId],
) -> AppResult<HashMap<AttemptId, (Score, u32)>> {
    let ids_json = serde_json::to_string(ids).expect("a list of IDs serializes");
    let mut statement = conn.prepare(
        "SELECT r.attempt_id, r.choice, q.answer_id, q.annulled
         FROM responses r JOIN questions q ON q.id = r.question_id
         WHERE r.attempt_id IN (SELECT value FROM json_each(?1))",
    )?;
    let mut rows = statement.query(params![ids_json])?;
    let mut scores: HashMap<AttemptId, (Score, u32)> = HashMap::new();
    while let Some(row) = rows.next()? {
        let choice: Option<String> = row.get(1)?;
        let answer_id: String = row.get(2)?;
        let annulled: bool = row.get(3)?;
        let entry = scores.entry(row.get(0)?).or_default();
        entry.0.record(outcome(
            choice.as_deref(),
            &answer_id,
            QuestionStatus::from(annulled),
        ));
        entry.1 += 1;
    }
    Ok(scores)
}
