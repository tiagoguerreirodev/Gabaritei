use super::*;

use super::view::title;
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

/// Columns of the unfinished-attempts query.
type ActiveRow = (
    AttemptId,
    SourceKind,
    Option<String>,
    FeedbackMode,
    String,
    u32,
    u32,
);

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
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })?
        .collect::<Result<_, _>>()?;
    rows.into_iter()
        .map(
            |(id, source_kind, source_ref, feedback_mode, started_at, answered, total)| {
                let source_ref = SourceRef::from_storage(source_kind, source_ref);
                Ok(ActiveAttempt {
                    title: title(conn, source_kind, source_ref.as_ref())?,
                    id,
                    source_kind,
                    feedback_mode,
                    started_at,
                    answered,
                    total,
                })
            },
        )
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
