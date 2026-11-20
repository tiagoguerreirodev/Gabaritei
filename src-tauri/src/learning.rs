//! Error notebook and spaced repetition (1 → 3 → 7 → 14 → 30 → 60 days).
//!
//! Filled in by issue #21 (spec §3.5).

use rusqlite::Connection;

use crate::error::AppResult;

/// Questions for an error-notebook practice session. Filled in by #21.
pub fn error_questions(_conn: &Connection) -> AppResult<Vec<String>> {
    Ok(Vec::new())
}

/// Questions whose spaced-repetition review is due at `now`. Filled in by #21.
pub fn due_questions(_conn: &Connection, _now: &str) -> AppResult<Vec<String>> {
    Ok(Vec::new())
}
