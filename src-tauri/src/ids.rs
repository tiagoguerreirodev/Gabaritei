//! Database identities use the same scalar values in SQLite, JSON and the frontend.

use std::fmt;

use rand::Rng;
use rusqlite::ToSql;
use rusqlite::types::{FromSql, FromSqlResult, ToSqlOutput, ValueRef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

macro_rules! scalar_id {
    ($name:ident, $inner:ty $(, $derive:ident)*) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS, JsonSchema $(, $derive)*)]
        #[serde(transparent)]
        #[ts(export)]
        pub struct $name($inner);

        impl From<$inner> for $name {
            fn from(value: $inner) -> Self {
                Self(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, formatter)
            }
        }

        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                self.0.to_sql()
            }
        }

        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                <$inner>::column_result(value).map(Self)
            }
        }
    };
}

macro_rules! text_id {
    ($name:ident) => {
        scalar_id!($name, String);

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

text_id!(AttemptId);
text_id!(QuestionId);
text_id!(CollectionId);
text_id!(DraftId);
scalar_id!(TopicId, i64, Copy);

/// Attempts, drafts and generated collections share the existing 32-digit hex ID format.
pub(crate) fn generate<T: From<String>>(rng: &mut impl Rng) -> T {
    let bytes: [u8; 16] = rng.random();
    hex::encode(bytes).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};

    #[test]
    fn generated_ids_are_repeatable_with_the_same_seed() {
        let first: AttemptId = generate(&mut StdRng::seed_from_u64(17));
        let second: DraftId = generate(&mut StdRng::seed_from_u64(17));
        assert_eq!(first.as_str(), second.as_str());
        assert_eq!(first.as_str().len(), 32);
        assert!(first.as_str().bytes().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn sqlite_and_json_preserve_scalar_ids() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let question = QuestionId::from("legacy-question");
        let topic = TopicId::from(17);
        let stored = conn
            .query_row("SELECT ?1, ?2", rusqlite::params![question, topic], |row| {
                Ok((row.get::<_, QuestionId>(0)?, row.get::<_, TopicId>(1)?))
            })
            .unwrap();
        assert_eq!(
            serde_json::to_value(stored).unwrap(),
            serde_json::json!(["legacy-question", 17])
        );
    }
}
