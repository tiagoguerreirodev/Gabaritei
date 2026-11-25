//! Question bank: question model and validation, identity, deduplicating import, and the
//! generated-package format (spec §3.1).
//!
//! Contest and board tags live on collections, not on questions: a question's contests are
//! those of the collections that contain it (owner decision, 2026-11-19).

pub mod catalog;
pub mod identity;
pub mod package;
pub mod question;
pub mod store;
pub(crate) mod tags;

pub use package::{Package, PackageQuestion, package_json_schema};
pub use question::{
    Provenance, Question, QuestionInput, QuestionOption, QuestionType, TopicPath, canonicalize,
    validate,
};
pub use store::{ImportOutcome, import_questions, load_question};
