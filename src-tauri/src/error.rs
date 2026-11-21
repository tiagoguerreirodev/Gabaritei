//! Error type returned by every Tauri command.
//!
//! The frontend receives an [`ErrorPayload`] whose `message` is pt-BR text that can be
//! shown to the user as is. Technical details never reach the frontend.

use serde::{Serialize, Serializer};
use ts_rs::TS;

/// Errors a command can return.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// A problem the user can act on. The message is pt-BR and shown as is.
    #[error("{0}")]
    Invalid(String),
    /// An unexpected failure. The detail is for logs; the user sees a generic message.
    #[error("Ocorreu um erro inesperado. Tente novamente.")]
    Internal(String),
}

/// What the frontend receives when a command fails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ErrorPayload {
    /// pt-BR message, safe to show to the user.
    pub message: String,
}

impl From<&AppError> for ErrorPayload {
    fn from(error: &AppError) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

impl AppError {
    /// The technical text for logs: the detail of an unexpected failure, or the user message.
    /// It never holds stored values (see the `From` impls below).
    pub fn detail(&self) -> &str {
        match self {
            Self::Invalid(message) | Self::Internal(message) => message,
        }
    }
}

impl Serialize for AppError {
    /// Every error that reaches the frontend passes through here, so an unexpected one is
    /// written to the app log first: the user only sees a generic message.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Self::Internal(detail) = self {
            crate::log::error(format!("unexpected error: {detail}"));
        }
        ErrorPayload::from(self).serialize(serializer)
    }
}

/// Result type for domain functions and commands.
pub type AppResult<T> = Result<T, AppError>;

/// Unexpected failures from libraries become [`AppError::Internal`]. The text is kept for the
/// log, so it must not echo stored values: SQLite and JSON messages can quote the data that
/// failed, and those two are described without the quoted part.
macro_rules! internal_from {
    ($($source:ty),* $(,)?) => {
        $(impl From<$source> for AppError {
            fn from(error: $source) -> Self {
                Self::Internal(error.to_string())
            }
        })*
    };
}

internal_from!(std::io::Error);

impl From<serde_json::Error> for AppError {
    fn from(error: serde_json::Error) -> Self {
        Self::Internal(format!(
            "invalid JSON ({:?} at line {}, column {})",
            error.classify(),
            error.line(),
            error.column()
        ))
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Internal(match &error {
            // SQLite's own text names the failed constraint or file problem, not the values.
            rusqlite::Error::SqliteFailure(code, message) => format!(
                "database error {:?}: {}",
                code.code,
                message.as_deref().unwrap_or("no message")
            ),
            rusqlite::Error::QueryReturnedNoRows => "database query returned no rows".into(),
            // Other variants can carry a value or a nested error with one.
            _ => "database error (detail withheld: it may contain stored values)".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn invalid_error_serializes_its_own_message() {
        let error = AppError::Invalid("Informe um assunto.".into());
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            json!({ "message": "Informe um assunto." })
        );
    }

    #[test]
    fn the_detail_of_a_json_error_never_echoes_the_content() {
        #[derive(Debug, serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Shape {
            _known: u32,
        }
        let error = serde_json::from_str::<Shape>(r#"{"SEGREDO-DA-QUESTAO": 1}"#).unwrap_err();
        assert!(error.to_string().contains("SEGREDO-DA-QUESTAO"), "premise");
        let app: AppError = error.into();
        assert!(
            !app.detail().contains("SEGREDO-DA-QUESTAO"),
            "{}",
            app.detail()
        );
        assert!(app.detail().contains("JSON"));
    }

    #[test]
    fn the_detail_of_a_database_error_names_the_failure_not_the_values() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE t (id TEXT PRIMARY KEY)", [])
            .unwrap();
        conn.execute("INSERT INTO t VALUES ('VALOR-GUARDADO')", [])
            .unwrap();
        let error = conn
            .execute("INSERT INTO t VALUES ('VALOR-GUARDADO')", [])
            .unwrap_err();
        let app: AppError = error.into();
        assert!(app.detail().contains("UNIQUE"), "{}", app.detail());
        assert!(!app.detail().contains("VALOR-GUARDADO"));
    }

    #[test]
    fn detail_is_the_message_for_invalid_errors() {
        assert_eq!(
            AppError::Invalid("Informe um assunto.".into()).detail(),
            "Informe um assunto."
        );
    }

    #[test]
    fn internal_error_hides_the_detail() {
        let error = AppError::Internal("disk I/O error at /secret/path".into());
        let value = serde_json::to_value(&error).unwrap();
        assert_eq!(
            value,
            json!({ "message": "Ocorreu um erro inesperado. Tente novamente." })
        );
        assert!(!value.to_string().contains("/secret/path"));
    }
}
