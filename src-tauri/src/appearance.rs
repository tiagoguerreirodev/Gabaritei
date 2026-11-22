//! Look-and-feel preferences: colour theme and text size.
//!
//! Stored as one JSON setting. A missing or unreadable value means the defaults, so a bad
//! value can never stop the app from showing its UI.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::db;
use crate::error::AppResult;

pub const KEY: &str = "appearance";

/// Colour theme. `System` follows the operating system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// Text size step: small, medium (default), large, extra large.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum TextSize {
    S,
    #[default]
    M,
    L,
    Xl,
}

/// The saved appearance preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Appearance {
    pub theme: Theme,
    pub text_size: TextSize,
}

/// Reads the preferences; the defaults when never saved or when the stored value is invalid.
pub fn load(conn: &Connection) -> AppResult<Appearance> {
    Ok(db::get_raw_setting(conn, KEY)?
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default())
}

/// Saves the preferences and returns them.
pub fn save(conn: &Connection, appearance: Appearance) -> AppResult<Appearance> {
    db::set_setting(conn, KEY, &appearance)?;
    Ok(appearance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn defaults_to_system_theme_and_medium_text() {
        let db = Database::open_in_memory().unwrap();
        let appearance = db.with(|conn| load(conn)).unwrap();
        assert_eq!(appearance.theme, Theme::System);
        assert_eq!(appearance.text_size, TextSize::M);
    }

    #[test]
    fn saved_preferences_are_read_back() {
        let db = Database::open_in_memory().unwrap();
        let wanted = Appearance {
            theme: Theme::Dark,
            text_size: TextSize::Xl,
        };
        db.with(|conn| save(conn, wanted)).unwrap();
        assert_eq!(db.with(|conn| load(conn)).unwrap(), wanted);
    }

    #[test]
    fn an_unreadable_stored_value_falls_back_to_the_defaults() {
        let db = Database::open_in_memory().unwrap();
        db.with(|conn| db::put_raw_setting(conn, KEY, Some("{\"theme\":\"neon\"}")))
            .unwrap();
        assert_eq!(db.with(|conn| load(conn)).unwrap(), Appearance::default());
    }

    #[test]
    fn a_partial_stored_value_keeps_its_valid_fields() {
        let db = Database::open_in_memory().unwrap();
        db.with(|conn| db::put_raw_setting(conn, KEY, Some("{\"theme\":\"dark\"}")))
            .unwrap();
        let loaded = db.with(|conn| load(conn)).unwrap();
        assert_eq!(loaded.theme, Theme::Dark);
        assert_eq!(loaded.text_size, TextSize::M);
    }

    #[test]
    fn serialises_as_lowercase_camel_case() {
        let json = serde_json::to_value(Appearance {
            theme: Theme::Light,
            text_size: TextSize::Xl,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "theme": "light", "textSize": "xl" })
        );
    }
}
