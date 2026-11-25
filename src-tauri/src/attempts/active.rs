use rusqlite::{Connection, params};
use serde::Serialize;
use ts_rs::TS;

use super::{ANSWERED_RESPONSE_SQL, FeedbackMode, SourceKind, SourceRef, load_header, view::title};
use crate::error::{AppError, AppResult};
use crate::ids::AttemptId;

/// An unfinished attempt, for the "resume" list on the home page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ActiveAttempt {
    pub id: AttemptId,
    pub title: String,
    pub source_kind: SourceKind,
    pub feedback_mode: FeedbackMode,
    pub started_at: String,
    /// Questions answered so far.
    pub answered: u32,
    pub total: u32,
}

/// Every unfinished attempt, newest first. Carries counts only, never questions or keys.
pub fn active(conn: &Connection) -> AppResult<Vec<ActiveAttempt>> {
    let sql = format!(
        "SELECT a.id, a.source_kind, a.source_ref, a.feedback_mode, a.started_at,
                COUNT(*) FILTER (WHERE {ANSWERED_RESPONSE_SQL}), COUNT(r.position)
         FROM attempts a LEFT JOIN responses r ON r.attempt_id = a.id
         WHERE a.finished_at IS NULL
         GROUP BY a.id ORDER BY a.started_at DESC, a.id",
    );
    let rows: Vec<ActiveRow> = conn
        .prepare(&sql)?
        .query_map([], ActiveRow::from_row)?
        .collect::<Result<_, _>>()?;
    rows.into_iter()
        .map(|row| {
            let source_ref = SourceRef::from_storage(row.source_kind, row.stored_ref);
            Ok(ActiveAttempt {
                title: title(conn, row.source_kind, source_ref.as_ref())?,
                id: row.id,
                source_kind: row.source_kind,
                feedback_mode: row.mode,
                started_at: row.started_at,
                answered: row.answered,
                total: row.total,
            })
        })
        .collect()
}

/// Deletes an unfinished attempt. Finished attempts are history and are kept forever.
pub fn abandon(conn: &Connection, id: &AttemptId) -> AppResult<()> {
    let header = load_header(conn, id)?;
    if header.finished_at.is_some() {
        return Err(AppError::Invalid(
            "Tentativas finalizadas fazem parte do histórico e não podem ser apagadas.".into(),
        ));
    }
    conn.execute("DELETE FROM attempts WHERE id = ?1", params![id])?;
    Ok(())
}

struct ActiveRow {
    id: AttemptId,
    source_kind: SourceKind,
    stored_ref: Option<String>,
    mode: FeedbackMode,
    started_at: String,
    answered: u32,
    total: u32,
}

impl ActiveRow {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            source_kind: row.get(1)?,
            stored_ref: row.get(2)?,
            mode: row.get(3)?,
            started_at: row.get(4)?,
            answered: row.get(5)?,
            total: row.get(6)?,
        })
    }
}
