//! The package the AI generator returns (spec §3.2, §4.1).
//!
//! The JSON Schema generated from these types is what the AI runner hands to the CLIs
//! (#14, #16). The app assigns the topic, so questions in a package carry none.
//! A snapshot of the schema lives in `src-tauri/schemas/package.schema.json`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::question::{Provenance, QuestionInput, QuestionOption, QuestionType, TopicPath};
use crate::error::{AppError, AppResult};

/// Questions written by the generator for one topic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Pacote de questões")]
pub struct Package {
    pub questions: Vec<PackageQuestion>,
    /// Remarks from the generator about the package (sources used, caveats).
    pub notes: Vec<String>,
}

/// One generated question. Option IDs are local; the app replaces them with canonical IDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PackageQuestion {
    /// Unique within the package, e.g. "q1".
    pub id: String,
    #[serde(rename = "type")]
    pub kind: QuestionType,
    pub stem: String,
    /// Five options for `multiple`; exactly "Certo" and "Errado" for `boolean`.
    pub options: Vec<QuestionOption>,
    /// ID of the correct option.
    pub answer_id: String,
    pub explanation: String,
    /// Verifiable sources (law, article, official page). Never invented.
    pub refs: Vec<String>,
}

impl Package {
    /// Parses a package, rejecting anything that doesn't match the schema exactly.
    pub fn parse(json: &str) -> AppResult<Self> {
        serde_json::from_str(json).map_err(|error| {
            AppError::Invalid(format!("O pacote não está no formato esperado: {error}."))
        })
    }

    /// Turns the package into question inputs for `topic`.
    pub fn into_inputs(self, topic: &TopicPath, provenance: &Provenance) -> Vec<QuestionInput> {
        self.questions
            .into_iter()
            .map(|q| QuestionInput {
                local_id: q.id,
                kind: q.kind,
                stem: q.stem,
                options: q.options,
                answer_id: q.answer_id,
                explanation: q.explanation,
                refs: q.refs,
                topics: vec![topic.clone()],
                image: None,
                image_alt: None,
                annulled: false,
                provenance: provenance.clone(),
            })
            .collect()
    }
}

/// JSON Schema for [`Package`], as handed to the AI CLIs.
pub fn package_json_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Package)).expect("schema serializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bank::question::tests::topic;
    use crate::bank::store::import_questions;
    use crate::db::Database;
    use serde_json::json;
    use std::path::Path;

    const SNAPSHOT: &str = "schemas/package.schema.json";

    fn sample() -> serde_json::Value {
        json!({
            "questions": [
                {
                    "id": "q1",
                    "type": "boolean",
                    "stem": "A crase indica a fusão da preposição \"a\" com o artigo \"a\".",
                    "options": [{ "id": "c", "text": "Certo" }, { "id": "e", "text": "Errado" }],
                    "answerId": "c",
                    "explanation": "Fusão de preposição e artigo.",
                    "refs": ["Bechara, Moderna Gramática Portuguesa"]
                }
            ],
            "notes": []
        })
    }

    #[test]
    fn schema_matches_the_snapshot() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(SNAPSHOT);
        let generated = serde_json::to_string_pretty(&package_json_schema()).unwrap() + "\n";
        if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &generated).unwrap();
        }
        let stored = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            stored, generated,
            "{SNAPSHOT} is out of date. If the change is intended, run \
             `UPDATE_SNAPSHOTS=1 cargo test` and commit the file."
        );
    }

    #[test]
    fn schema_is_strict() {
        let schema = package_json_schema();
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(schema["required"], json!(["questions", "notes"]));
        let question = &schema["$defs"]["PackageQuestion"];
        assert_eq!(question["additionalProperties"], json!(false));
        assert_eq!(
            question["required"],
            json!([
                "id",
                "type",
                "stem",
                "options",
                "answerId",
                "explanation",
                "refs"
            ])
        );
    }

    #[test]
    fn a_valid_package_is_assigned_the_requested_topic() {
        let package = Package::parse(&sample().to_string()).unwrap();
        let inputs = package.into_inputs(&topic(), &Provenance::Legacy);
        assert_eq!(inputs[0].topics, vec![topic()]);
    }

    #[test]
    fn a_valid_package_is_importable() {
        let package = Package::parse(&sample().to_string()).unwrap();
        let provenance = Provenance::Generated {
            draft_id: "d1".into(),
            disputed: false,
        };
        let inputs = package.into_inputs(&topic(), &provenance);
        let db = Database::open_in_memory().unwrap();
        let outcome = db
            .transaction(|tx| import_questions(tx, inputs, "2026-11-19T00:00:00.000Z"))
            .unwrap();
        assert_eq!(outcome.inserted, 1);
    }

    #[test]
    fn structurally_invalid_packages_are_rejected() {
        let mut extra_field = sample();
        extra_field["questions"][0]["difficulty"] = json!("média");
        let mut missing_field = sample();
        missing_field["questions"][0]
            .as_object_mut()
            .unwrap()
            .remove("answerId");
        let mut wrong_type = sample();
        wrong_type["questions"][0]["type"] = json!("essay");
        let mut missing_notes = sample();
        missing_notes.as_object_mut().unwrap().remove("notes");

        for (name, value) in [
            ("extra field", extra_field),
            ("missing field", missing_field),
            ("wrong type", wrong_type),
            ("missing notes", missing_notes),
        ] {
            let error = Package::parse(&value.to_string()).expect_err(name);
            assert!(
                error
                    .to_string()
                    .starts_with("O pacote não está no formato esperado"),
                "{name}: {error}"
            );
        }
        assert!(Package::parse("not json").is_err());
    }
}
