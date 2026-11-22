//! Whether the single-letter keyboard shortcuts of the Resolver are on (default: on).
//!
//! A local setting like the appearance: it describes this computer, so a restore or import
//! from another computer never overwrites it.

use rusqlite::Connection;

use crate::db;
use crate::error::AppResult;

pub const KEY: &str = "keyboardShortcuts";

/// Reads the setting; on when never saved or when the stored value is invalid.
pub fn load(conn: &Connection) -> AppResult<bool> {
    Ok(db::get_raw_setting(conn, KEY)?
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or(true))
}

/// Saves the setting and returns it.
pub fn save(conn: &Connection, enabled: bool) -> AppResult<bool> {
    db::set_setting(conn, KEY, &enabled)?;
    Ok(enabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn on_by_default_then_saved_value_is_read_back() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.with(|conn| load(conn)).unwrap());
        db.with(|conn| save(conn, false)).unwrap();
        assert!(!db.with(|conn| load(conn)).unwrap());
        db.with(|conn| save(conn, true)).unwrap();
        assert!(db.with(|conn| load(conn)).unwrap());
    }

    #[test]
    fn an_unreadable_stored_value_means_on() {
        let db = Database::open_in_memory().unwrap();
        db.with(|conn| db::put_raw_setting(conn, KEY, Some("\"maybe\"")))
            .unwrap();
        assert!(db.with(|conn| load(conn)).unwrap());
    }

    #[test]
    fn is_a_local_setting() {
        assert!(crate::backup::LOCAL_SETTINGS.contains(&KEY));
    }
}
