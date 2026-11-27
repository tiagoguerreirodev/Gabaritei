//! Sessions and attempts: starting, answering, eliminating, flagging, finishing, abandoning,
//! and the one home for outcome and scoring rules (spec §3.3, §3.4).
//!
//! **No spoilers (AGENTS.md rule 1).** [`AttemptView`] is the only shape an attempt takes on
//! its way to the frontend. A question's key, explanation and references live in
//! [`Reveal`], which is filled in only once that question is revealed: right after it is
//! answered in Study mode, or when the attempt is finished. Everything else in the view is
//! what the question card shows anyway.
//!
//! **Retroactive scoring.** A response stores a snapshot of the question *as shown* (stem and
//! shuffled options) and the choice. Key, explanation and annulment are always read from the
//! current question, so corrections and annulments re-score past attempts. No score is stored.

use rusqlite::types::ToSqlOutput;
use rusqlite::{Connection, OptionalExtension, ToSql, params};
use serde::ser::Error as _;
use serde::{Deserialize, Serialize, Serializer};
use ts_rs::TS;

use crate::bank::{QuestionOption, QuestionType};
use crate::error::{AppError, AppResult};
use crate::ids::{AttemptId, CollectionId, QuestionId, TopicId};
use crate::learning::ErrorFilter;
use crate::selection::{QuestionFilter, SessionOrder};

mod active;
mod closing;
mod history;
mod response;
mod scoring;
mod session;
mod view;

pub use active::{ActiveAttempt, abandon, active};
pub use closing::{AttemptClosing, PreviousAttempt, TopicScore, closing};
pub use history::{HistoryItem, HistoryPage, HistoryQuery, HistorySource, history};
pub use response::{annul, answer, eliminate, finish, flag, restore, revealed_question};
pub(crate) use scoring::{ANSWERED_RESPONSE_SQL, HIT_SQL, MISS_SQL};
pub use scoring::{Outcome, QuestionStatus, RESPONSE_TIME_SQL, Score, outcome};
pub use session::start;
pub use view::load;

/// Largest topic session.
pub const MAX_TOPIC_SESSION: u32 = 200;

/// Largest session started from a filter.
pub const MAX_SELECTION_SESSION: u32 = 200;

// ---- Requests --------------------------------------------------------------------------------

/// *Estudo* reveals each answer right away; *Simulado* reveals everything on finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum FeedbackMode {
    Study,
    Exam,
}

crate::enum_text::stored_enum!(FeedbackMode {
    Study => "study",
    Exam => "exam",
});

/// Where a session's questions come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum SessionSource {
    /// Every question of a collection.
    Collection { collection_id: CollectionId },
    /// Up to `count` questions of a topic: unseen first, then least recently answered.
    Topic { topic_id: TopicId, count: u32 },
    /// The error notebook, most recent errors first (up to 100). Blanks join when asked.
    Errors {
        #[serde(rename = "includeBlanks")]
        #[ts(type = "boolean")]
        filter: ErrorFilter,
    },
    /// Due reviews (most overdue first), then recent errors (up to 100 in all).
    Reviews,
    /// Up to `count` questions that match a combined filter, picked in the given order.
    Filter {
        filter: QuestionFilter,
        count: u32,
        order: SessionOrder,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SessionStart {
    pub source: SessionSource,
    pub feedback_mode: FeedbackMode,
}

// ---- Views (what the frontend may see) ---------------------------------------------------------

/// Key, explanation and result of a revealed question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Reveal {
    pub answer_id: String,
    pub explanation: String,
    pub refs: Vec<String>,
    pub annulled: bool,
    pub outcome: Outcome,
    /// The blind check still disagreed with the key when this generated question was imported.
    pub disputed: bool,
}

/// One question of an attempt, as the card shows it. `reveal` is `None` until revealed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ResponseView {
    pub position: u32,
    pub question_id: QuestionId,
    #[serde(rename = "type")]
    #[ts(rename = "type", type = "\"multiple\" | \"boolean\"")]
    pub kind: QuestionType,
    pub stem: String,
    /// In the order shown in this attempt.
    #[ts(type = "Array<{ id: string, text: string }>")]
    pub options: Vec<QuestionOption>,
    pub image: Option<String>,
    pub image_alt: Option<String>,
    pub choice: Option<String>,
    pub eliminated: Vec<String>,
    pub flagged: bool,
    pub reveal: Option<Reveal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SourceKind {
    Collection,
    Topic,
    Errors,
    Reviews,
    Filter,
}

crate::enum_text::stored_enum!(SourceKind {
    Collection => "collection",
    Topic => "topic",
    Errors => "errors",
    Reviews => "reviews",
    Filter => "filter",
});

/// A source stored as text, including topic IDs for compatibility with existing attempts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRef {
    Collection(CollectionId),
    Topic(TopicId),
    Filter(QuestionFilter),
}

impl SourceRef {
    fn from_storage(kind: SourceKind, stored: Option<String>) -> Option<Self> {
        let stored = stored?;
        match kind {
            SourceKind::Collection => Some(Self::Collection(stored.into())),
            SourceKind::Topic => stored.parse::<i64>().ok().map(|id| Self::Topic(id.into())),
            SourceKind::Filter => serde_json::from_str(&stored).ok().map(Self::Filter),
            SourceKind::Errors | SourceKind::Reviews => None,
        }
    }
}

impl Serialize for SourceRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Collection(id) => id.serialize(serializer),
            Self::Topic(id) => serializer.collect_str(id),
            Self::Filter(filter) => {
                serializer.collect_str(&serde_json::to_string(filter).map_err(S::Error::custom)?)
            }
        }
    }
}

impl ToSql for SourceRef {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        match self {
            Self::Collection(id) => id.to_sql(),
            Self::Topic(id) => Ok(id.to_string().into()),
            Self::Filter(filter) => serde_json::to_string(filter)
                .map(Into::into)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AttemptView {
    pub id: AttemptId,
    pub source_kind: SourceKind,
    /// Collection ID or topic ID, depending on `source_kind`.
    #[ts(type = "string | null")]
    pub source_ref: Option<SourceRef>,
    /// pt-BR title: the collection title, the topic path, "Caderno de erros" or "Revisões".
    pub title: String,
    pub feedback_mode: FeedbackMode,
    pub started_at: String,
    pub finished_at: Option<String>,
    /// Bumped on every change; `finish` must quote the latest one.
    pub revision: u32,
    pub questions: Vec<ResponseView>,
    /// Filled in once the attempt is finished.
    pub score: Option<Score>,
}

// ---- Storage ---------------------------------------------------------------------------------

/// The question as shown in an attempt. Holds no key, explanation or annulment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    #[serde(rename = "type")]
    kind: QuestionType,
    stem: String,
    options: Vec<QuestionOption>,
    image: Option<String>,
    image_alt: Option<String>,
}

struct Header {
    source_kind: SourceKind,
    source_ref: Option<SourceRef>,
    mode: FeedbackMode,
    started_at: String,
    finished_at: Option<String>,
    revision: u32,
}

fn not_found() -> AppError {
    AppError::Invalid("Tentativa não encontrada.".into())
}

fn load_header(conn: &Connection, id: &AttemptId) -> AppResult<Header> {
    let row = conn
        .query_row(
            "SELECT source_kind, source_ref, feedback_mode, started_at, finished_at, revision
             FROM attempts WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, SourceKind>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, FeedbackMode>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, u32>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(not_found)?;
    Ok(Header {
        source_kind: row.0,
        source_ref: SourceRef::from_storage(row.0, row.1),
        mode: row.2,
        started_at: row.3,
        finished_at: row.4,
        revision: row.5,
    })
}

#[cfg(test)]
mod tests;
