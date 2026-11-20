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

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ErrorPayload::from(self).serialize(serializer)
    }
}

/// Result type for domain functions and commands.
pub type AppResult<T> = Result<T, AppError>;

/// Unexpected failures from libraries become [`AppError::Internal`], keeping the detail.
macro_rules! internal_from {
    ($($source:ty),* $(,)?) => {
        $(impl From<$source> for AppError {
            fn from(error: $source) -> Self {
                Self::Internal(error.to_string())
            }
        })*
    };
}

internal_from!(rusqlite::Error, std::io::Error, serde_json::Error);

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
