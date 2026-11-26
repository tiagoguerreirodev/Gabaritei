//! Text representations for enums stored in SQLite.

#[derive(Debug, thiserror::Error)]
#[error("unknown value for {0}")]
pub struct UnknownEnumValue(pub(crate) &'static str);

macro_rules! stored_enum {
    ($kind:ident { $($variant:ident => $stored:literal),+ $(,)? }) => {
        impl $kind {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $stored),+
                }
            }
        }

        impl std::str::FromStr for $kind {
            type Err = $crate::enum_text::UnknownEnumValue;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($stored => Ok(Self::$variant)),+,
                    _ => Err($crate::enum_text::UnknownEnumValue(stringify!($kind))),
                }
            }
        }

        impl rusqlite::ToSql for $kind {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(self.as_str().into())
            }
        }

        impl rusqlite::types::FromSql for $kind {
            fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
                value.as_str()?.parse().map_err(|error| rusqlite::types::FromSqlError::Other(Box::new(error)))
            }
        }
    };
}

pub(crate) use stored_enum;

#[cfg(test)]
mod tests {
    use std::fmt::Debug;
    use std::str::FromStr;

    use rusqlite::types::FromSql;
    use rusqlite::{Connection, ToSql};

    use crate::ai::draft::DraftStatus;
    use crate::attempts::{FeedbackMode, SourceKind};
    use crate::bank::QuestionType;
    use crate::bank::catalog::CollectionKind;

    fn assert_storage<T>(cases: &[(T, &str)])
    where
        T: FromSql + ToSql + FromStr + Copy + PartialEq + Debug,
        T::Err: Debug,
    {
        let conn = Connection::open_in_memory().unwrap();
        for &(value, stored) in cases {
            let row: (String, T) = conn
                .query_row("SELECT ?1, ?1", [value], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })
                .unwrap();
            assert_eq!(row, (stored.to_owned(), value));
            assert_eq!(stored.parse::<T>().unwrap(), value);
        }
    }

    #[test]
    fn feedback_modes_round_trip_as_the_existing_text() {
        assert_storage(&[(FeedbackMode::Study, "study"), (FeedbackMode::Exam, "exam")]);
    }

    #[test]
    fn question_types_round_trip_as_the_existing_text() {
        assert_storage(&[
            (QuestionType::Multiple, "multiple"),
            (QuestionType::Boolean, "boolean"),
        ]);
    }

    #[test]
    fn collection_kinds_round_trip_as_the_existing_text() {
        assert_storage(&[
            (CollectionKind::Imported, "imported"),
            (CollectionKind::Generated, "generated"),
            (CollectionKind::Practice, "practice"),
        ]);
    }

    #[test]
    fn source_kinds_round_trip_as_the_existing_text() {
        assert_storage(&[
            (SourceKind::Collection, "collection"),
            (SourceKind::Topic, "topic"),
            (SourceKind::Errors, "errors"),
            (SourceKind::Reviews, "reviews"),
            (SourceKind::Filter, "filter"),
        ]);
    }

    #[test]
    fn draft_statuses_round_trip_as_the_existing_text() {
        assert_storage(&[
            (DraftStatus::Review, "review"),
            (DraftStatus::Imported, "imported"),
            (DraftStatus::Discarded, "discarded"),
        ]);
    }
}
