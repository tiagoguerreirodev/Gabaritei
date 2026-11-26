//! Question model, validation and canonicalization (spec §3.1).
//!
//! Questions arrive as [`QuestionInput`] (from a generated package or the legacy migration)
//! and are stored as canonical [`Question`]s: option IDs derived from their text and the ID
//! set to the content fingerprint (see [`super::identity`]).
//!
//! These types carry the answer key, so they must never be sent to the frontend as they are
//! (AGENTS.md rule 1). Frontend views get their own types.

use std::collections::HashSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::identity::{fingerprint, js_trim, option_id};
use crate::ids::{DraftId, QuestionId};
use crate::images::ImageFormat;

/// Number of options in a multiple-choice question.
const MULTIPLE_OPTIONS: usize = 5;
/// The two options of a Certo/Errado question.
const BOOLEAN_TEXTS: [&str; 2] = ["Certo", "Errado"];

const MAX_LOCAL_ID: usize = 200;
const MAX_STEM: usize = 100_000;
const MAX_OPTION_ID: usize = 100;
const MAX_OPTION_TEXT: usize = 20_000;
pub(crate) const MAX_EXPLANATION: usize = 100_000;
pub(crate) const MAX_REFS: usize = 50;
pub(crate) const MAX_REF: usize = 2_000;
const MIN_TOPICS: usize = 1;
const MAX_TOPICS: usize = 30;
pub(crate) const MAX_AREA: usize = 250;
pub(crate) const MAX_MATTER: usize = 250;
pub(crate) const MAX_TOPIC: usize = 500;
const MAX_IMAGE_ALT: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
/// `multiple`: five options, one correct. `boolean`: Certo/Errado.
pub enum QuestionType {
    Multiple,
    Boolean,
}

crate::enum_text::stored_enum!(QuestionType {
        Multiple => "multiple",
        Boolean => "boolean",
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionOption {
    pub id: String,
    pub text: String,
}

/// Area › matter › topic.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TopicPath {
    pub area: String,
    pub matter: String,
    pub topic: String,
}

impl std::fmt::Display for TopicPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} › {} › {}", self.area, self.matter, self.topic)
    }
}

impl TopicPath {
    pub fn new(area: &str, matter: &str, topic: &str) -> Self {
        Self {
            area: area.to_owned(),
            matter: matter.to_owned(),
            topic: topic.to_owned(),
        }
    }

    /// The same path with surrounding whitespace removed from each name.
    pub(super) fn trimmed(&self) -> Self {
        Self::new(
            js_trim(&self.area),
            js_trim(&self.matter),
            js_trim(&self.topic),
        )
    }
}

/// Where a question came from. Stored as JSON; later issues may add variants or fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "source",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Provenance {
    /// Imported from the legacy Caderno Local bank.
    Legacy,
    /// Written by the AI generator and approved from a draft.
    Generated {
        draft_id: DraftId,
        /// The blind check still disagreed with the key when the package was imported. The
        /// Resolver shows a badge after the user answers.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        disputed: bool,
    },
}

/// A question paired with its package label or its canonical content fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question<Id = QuestionId> {
    pub id: Id,
    pub kind: QuestionType,
    pub stem: String,
    pub options: Vec<QuestionOption>,
    pub answer_id: String,
    pub explanation: String,
    pub refs: Vec<String>,
    pub topics: Vec<TopicPath>,
    pub image: Option<String>,
    pub image_alt: Option<String>,
    pub annulled: bool,
    pub provenance: Provenance,
}

/// A question as received, with an identifier local to its input batch.
pub type QuestionInput = Question<String>;

impl<Id> Question<Id> {
    /// The text of the correct option.
    pub fn answer_text(&self) -> Option<&str> {
        self.options
            .iter()
            .find(|o| o.id == self.answer_id)
            .map(|o| o.text.as_str())
    }
}

fn is_blank(text: &str) -> bool {
    js_trim(text).is_empty()
}

fn too_long(text: &str, max: usize) -> bool {
    text.chars().count() > max
}

/// Whether a name matches the pattern allowed in the image store.
pub fn is_valid_image_name(name: &str) -> bool {
    ImageFormat::from_name(name).is_some()
}

/// A validation rule violation. Detailed import diagnostics may quote the question.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QuestionProblem {
    #[error("Identificador inválido (vazio ou com mais de {MAX_LOCAL_ID} caracteres).")]
    InvalidId,
    #[error("O enunciado está vazio.")]
    EmptyStem,
    #[error("O enunciado tem mais de {MAX_STEM} caracteres.")]
    LongStem,
    #[error("O comentário tem mais de {MAX_EXPLANATION} caracteres.")]
    LongExplanation,
    #[error("Questões de múltipla escolha têm {MULTIPLE_OPTIONS} alternativas; esta tem {0}.")]
    MultipleOptionCount(usize),
    #[error("Questões Certo/Errado têm exatamente as alternativas \"Certo\" e \"Errado\".")]
    BooleanOptions,
    #[error(
        "Alternativa com identificador inválido (vazio ou com mais de {MAX_OPTION_ID} caracteres)."
    )]
    InvalidOptionId,
    #[error("Há uma alternativa sem texto.")]
    EmptyOptionText,
    #[error("Uma alternativa tem mais de {MAX_OPTION_TEXT} caracteres.")]
    LongOptionText,
    #[error("Identificador de alternativa repetido: {0}.")]
    RepeatedOptionId(String),
    #[error("Alternativas com o mesmo texto: \"{0}\".")]
    RepeatedOptionText(String),
    #[error("O gabarito não corresponde a nenhuma alternativa.")]
    InvalidAnswer,
    #[error("Há mais de {MAX_REFS} referências.")]
    TooManyRefs,
    #[error("Há uma referência vazia.")]
    EmptyRef,
    #[error("Uma referência tem mais de {MAX_REF} caracteres.")]
    LongRef,
    #[error(
        "Cada questão precisa de {MIN_TOPICS} a {MAX_TOPICS} assuntos (área › matéria › assunto)."
    )]
    TopicCount,
    #[error("Área, matéria e assunto não podem ficar vazios.")]
    EmptyTopicName,
    #[error(
        "Área, matéria ou assunto longo demais (limites: {MAX_AREA}, {MAX_MATTER} e {MAX_TOPIC} caracteres)."
    )]
    LongTopicName,
    #[error("Nome de imagem inválido: {0}.")]
    ImageName(String),
    #[error(
        "A imagem precisa de uma descrição (texto alternativo) de até {MAX_IMAGE_ALT} caracteres."
    )]
    ImageDescription,
    #[error("Há texto alternativo, mas nenhuma imagem.")]
    DescriptionWithoutImage,
}

/// Every rule a question must follow. Returns pt-BR problems; empty means valid.
pub fn validate(input: &QuestionInput) -> Vec<QuestionProblem> {
    let mut problems = Vec::new();

    if is_blank(&input.id) || too_long(&input.id, MAX_LOCAL_ID) {
        problems.push(QuestionProblem::InvalidId);
    }
    if is_blank(&input.stem) {
        problems.push(QuestionProblem::EmptyStem);
    } else if too_long(&input.stem, MAX_STEM) {
        problems.push(QuestionProblem::LongStem);
    }

    check_options(input, &mut problems);
    if too_long(&input.explanation, MAX_EXPLANATION) {
        problems.push(QuestionProblem::LongExplanation);
    }
    check_references(&input.refs, &mut problems);
    check_topics(&input.topics, &mut problems);
    check_image(input, &mut problems);

    problems.dedup();
    problems
}

fn check_options(input: &QuestionInput, problems: &mut Vec<QuestionProblem>) {
    match input.kind {
        QuestionType::Multiple => {
            if input.options.len() != MULTIPLE_OPTIONS {
                problems.push(QuestionProblem::MultipleOptionCount(input.options.len()));
            }
        }
        QuestionType::Boolean => {
            let mut texts: Vec<&str> = input.options.iter().map(|o| o.text.as_str()).collect();
            texts.sort_unstable();
            if texts != BOOLEAN_TEXTS {
                problems.push(QuestionProblem::BooleanOptions);
            }
        }
    }

    let mut ids = HashSet::new();
    let mut texts = HashSet::new();
    for option in &input.options {
        if is_blank(&option.id) || too_long(&option.id, MAX_OPTION_ID) {
            problems.push(QuestionProblem::InvalidOptionId);
        }
        if is_blank(&option.text) {
            problems.push(QuestionProblem::EmptyOptionText);
        } else if too_long(&option.text, MAX_OPTION_TEXT) {
            problems.push(QuestionProblem::LongOptionText);
        }
        if !ids.insert(option.id.as_str()) {
            problems.push(QuestionProblem::RepeatedOptionId(option.id.clone()));
        }
        if !texts.insert(js_trim(&option.text).to_lowercase()) {
            problems.push(QuestionProblem::RepeatedOptionText(
                js_trim(&option.text).to_owned(),
            ));
        }
    }
    if !input.options.iter().any(|o| o.id == input.answer_id) {
        problems.push(QuestionProblem::InvalidAnswer);
    }
}

fn check_references(refs: &[String], problems: &mut Vec<QuestionProblem>) {
    if refs.len() > MAX_REFS {
        problems.push(QuestionProblem::TooManyRefs);
    }
    if refs.iter().any(|r| is_blank(r)) {
        problems.push(QuestionProblem::EmptyRef);
    }
    if refs.iter().any(|r| too_long(r, MAX_REF)) {
        problems.push(QuestionProblem::LongRef);
    }
}

fn check_topics(topics: &[TopicPath], problems: &mut Vec<QuestionProblem>) {
    if !(MIN_TOPICS..=MAX_TOPICS).contains(&topics.len()) {
        problems.push(QuestionProblem::TopicCount);
    }
    for path in topics {
        let names = [
            (&path.area, MAX_AREA),
            (&path.matter, MAX_MATTER),
            (&path.topic, MAX_TOPIC),
        ];
        if names.iter().any(|(name, _)| is_blank(name)) {
            problems.push(QuestionProblem::EmptyTopicName);
        }
        if names.iter().any(|(name, max)| too_long(name, *max)) {
            problems.push(QuestionProblem::LongTopicName);
        }
    }
}

fn check_image(input: &QuestionInput, problems: &mut Vec<QuestionProblem>) {
    match (&input.image, &input.image_alt) {
        (Some(image), alt) => {
            if !is_valid_image_name(image) {
                problems.push(QuestionProblem::ImageName(image.clone()));
            }
            match alt {
                Some(alt) if !is_blank(alt) && !too_long(alt, MAX_IMAGE_ALT) => {}
                _ => problems.push(QuestionProblem::ImageDescription),
            }
        }
        (None, Some(_)) => problems.push(QuestionProblem::DescriptionWithoutImage),
        (None, None) => {}
    }
}

/// Validates the input and turns it into a canonical [`Question`]: option IDs derived from
/// their text, the answer key mapped accordingly, topic names trimmed and deduplicated,
/// and the ID set to the content fingerprint.
pub fn canonicalize(input: QuestionInput) -> Result<Question, Vec<QuestionProblem>> {
    let problems = validate(&input);
    if !problems.is_empty() {
        return Err(problems);
    }
    let answer_id = option_id(
        input
            .answer_text()
            .expect("validated: the answer matches an option"),
    );
    let options: Vec<QuestionOption> = input
        .options
        .into_iter()
        .map(|o| QuestionOption {
            id: option_id(&o.text),
            text: o.text,
        })
        .collect();
    let mut topics = Vec::new();
    extend_unique(&mut topics, input.topics.iter().map(TopicPath::trimmed));
    let mut refs = Vec::new();
    extend_unique(&mut refs, input.refs);
    let id = fingerprint(
        input.kind,
        &input.stem,
        options.iter().map(|o| o.text.as_str()),
        input.image.as_deref(),
    );
    Ok(Question {
        id,
        kind: input.kind,
        stem: input.stem,
        answer_id,
        options,
        explanation: input.explanation,
        refs,
        topics,
        image: input.image,
        image_alt: input.image_alt,
        annulled: input.annulled,
        provenance: input.provenance,
    })
}

/// Adds unseen items while preserving the input order.
pub(super) fn extend_unique<T: PartialEq>(values: &mut Vec<T>, items: impl IntoIterator<Item = T>) {
    for item in items {
        if !values.contains(&item) {
            values.push(item);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn topic() -> TopicPath {
        TopicPath::new(
            "Direito",
            "Constitucional",
            "Controle de constitucionalidade",
        )
    }

    pub(crate) fn multiple(local_id: &str, stem: &str, answer: usize) -> QuestionInput {
        let options: Vec<QuestionOption> = ["Alfa", "Beta", "Gama", "Delta", "Épsilon"]
            .iter()
            .enumerate()
            .map(|(i, text)| QuestionOption {
                id: format!("o{i}"),
                text: (*text).to_owned(),
            })
            .collect();
        QuestionInput {
            id: local_id.to_owned(),
            kind: QuestionType::Multiple,
            stem: stem.to_owned(),
            answer_id: options[answer].id.clone(),
            options,
            explanation: String::new(),
            refs: vec![],
            topics: vec![topic()],
            image: None,
            image_alt: None,
            annulled: false,
            provenance: Provenance::Legacy,
        }
    }

    pub(crate) fn boolean(local_id: &str, stem: &str, certo: bool) -> QuestionInput {
        QuestionInput {
            id: local_id.to_owned(),
            kind: QuestionType::Boolean,
            stem: stem.to_owned(),
            options: vec![
                QuestionOption {
                    id: "c".into(),
                    text: "Certo".into(),
                },
                QuestionOption {
                    id: "e".into(),
                    text: "Errado".into(),
                },
            ],
            answer_id: if certo { "c" } else { "e" }.into(),
            explanation: String::new(),
            refs: vec![],
            topics: vec![topic()],
            image: None,
            image_alt: None,
            annulled: false,
            provenance: Provenance::Legacy,
        }
    }

    fn problems_of(input: QuestionInput) -> Vec<String> {
        validate(&input).iter().map(ToString::to_string).collect()
    }

    fn assert_rejected(input: QuestionInput, fragment: &str) {
        let problems = problems_of(input);
        assert!(
            problems.iter().any(|p| p.contains(fragment)),
            "expected a problem containing {fragment:?}, got {problems:?}"
        );
    }

    #[test]
    fn valid_questions_have_no_problems() {
        assert_eq!(
            problems_of(multiple("q1", "Enunciado", 2)),
            Vec::<String>::new()
        );
        assert_eq!(
            problems_of(boolean("q2", "Enunciado", true)),
            Vec::<String>::new()
        );
    }

    #[test]
    fn multiple_choice_needs_exactly_five_options() {
        let mut q = multiple("q", "Enunciado", 0);
        q.options.pop();
        assert_rejected(q, "têm 5 alternativas; esta tem 4");
    }

    #[test]
    fn boolean_requires_the_standard_option_texts() {
        let mut q = boolean("q", "Enunciado", true);
        q.options[1].text = "Falso".into();
        assert_rejected(q, "\"Certo\" e \"Errado\"");
    }

    #[test]
    fn boolean_refuses_a_third_option() {
        let mut q = boolean("q", "Enunciado", true);
        q.options.push(QuestionOption {
            id: "x".into(),
            text: "Talvez".into(),
        });
        assert_rejected(q, "\"Certo\" e \"Errado\"");
    }

    #[test]
    fn option_ids_must_be_distinct() {
        let mut q = multiple("q", "Enunciado", 0);
        q.options[1].id = "o0".into();
        assert_rejected(q, "Identificador de alternativa repetido");
    }

    #[test]
    fn option_texts_must_be_distinct() {
        let mut q = multiple("q", "Enunciado", 0);
        q.options[1].text = "  ALFA ".into();
        assert_rejected(q, "Alternativas com o mesmo texto");
    }

    #[test]
    fn answer_must_match_an_option() {
        let mut q = multiple("q", "Enunciado", 0);
        q.answer_id = "nenhuma".into();
        assert_rejected(q, "O gabarito não corresponde");
    }

    #[test]
    fn the_stem_cannot_be_blank() {
        assert_rejected(multiple("q", " \n ", 0), "O enunciado está vazio");
    }

    #[test]
    fn the_stem_has_a_length_limit() {
        assert_rejected(
            multiple("q", &"x".repeat(MAX_STEM + 1), 0),
            "O enunciado tem mais de",
        );
    }

    #[test]
    fn an_option_needs_text() {
        let mut q = multiple("q", "Enunciado", 0);
        q.options[3].text = "\u{a0}".into();
        assert_rejected(q, "alternativa sem texto");
    }

    #[test]
    fn an_option_has_a_length_limit() {
        let mut q = multiple("q", "Enunciado", 0);
        q.options[3].text = "y".repeat(MAX_OPTION_TEXT + 1);
        assert_rejected(q, "Uma alternativa tem mais de");
    }

    #[test]
    fn the_explanation_has_a_length_limit() {
        let mut q = multiple("q", "Enunciado", 0);
        q.explanation = "z".repeat(MAX_EXPLANATION + 1);
        assert_rejected(q, "O comentário tem mais de");
    }

    #[test]
    fn a_local_id_must_be_present() {
        assert_rejected(multiple("", "Enunciado", 0), "Identificador inválido");
    }

    #[test]
    fn the_reference_list_has_a_length_limit() {
        let mut q = multiple("q", "Enunciado", 0);
        q.refs = vec!["Lei 8.112/1990".into(); MAX_REFS + 1];
        assert_rejected(q, "Há mais de 50 referências");
    }

    #[test]
    fn a_reference_needs_text() {
        let mut q = multiple("q", "Enunciado", 0);
        q.refs = vec![" ".into()];
        assert_rejected(q, "referência vazia");
    }

    #[test]
    fn a_reference_has_a_length_limit() {
        let mut q = multiple("q", "Enunciado", 0);
        q.refs = vec!["r".repeat(MAX_REF + 1)];
        assert_rejected(q, "Uma referência tem mais de");
    }

    #[test]
    fn a_question_needs_at_least_one_topic() {
        let mut q = multiple("q", "Enunciado", 0);
        q.topics.clear();
        assert_rejected(q, "de 1 a 30 assuntos");
    }

    #[test]
    fn the_topic_list_has_a_length_limit() {
        let mut q = multiple("q", "Enunciado", 0);
        q.topics = (0..=MAX_TOPICS)
            .map(|i| TopicPath::new("A", "M", &format!("T{i}")))
            .collect();
        assert_rejected(q, "de 1 a 30 assuntos");
    }

    #[test]
    fn every_level_of_the_topic_path_needs_a_name() {
        let mut q = multiple("q", "Enunciado", 0);
        q.topics = vec![TopicPath::new("Direito", " ", "Crase")];
        assert_rejected(q, "não podem ficar vazios");
    }

    #[test]
    fn the_topic_area_name_has_a_length_limit() {
        let mut q = multiple("q", "Enunciado", 0);
        q.topics = vec![TopicPath::new(&"a".repeat(MAX_AREA + 1), "M", "T")];
        assert_rejected(q, "longo demais");
    }

    #[test]
    fn image_names_have_a_fixed_format() {
        assert!(is_valid_image_name("0123456789abcdef01234567.png"));
        assert!(is_valid_image_name(&format!("{}.webp", "a".repeat(64))));
        for bad in [
            "0123456789abcdef0123456.png",
            &format!("{}.png", "a".repeat(65)),
            "0123456789ABCDEF01234567.png",
            "0123456789abcdef01234567.gif",
            "../0123456789abcdef01234567.png",
            "0123456789abcdef01234567.png.exe",
        ] {
            assert!(!is_valid_image_name(bad), "{bad}");
        }
    }

    #[test]
    fn an_invalid_image_name_is_rejected() {
        let mut q = multiple("q", "Enunciado", 0);
        q.image = Some("../segredo.png".into());
        q.image_alt = Some("Figura".into());
        assert_rejected(q, "Nome de imagem inválido");
    }

    #[test]
    fn an_image_requires_alt_text() {
        let mut q = multiple("q", "Enunciado", 0);
        q.image = Some("0123456789abcdef0123456789abcdef.png".into());
        assert_rejected(q, "precisa de uma descrição");
    }

    #[test]
    fn alt_text_requires_an_image() {
        let mut q = multiple("q", "Enunciado", 0);
        q.image_alt = Some("Figura".into());
        assert_rejected(q, "nenhuma imagem");
    }

    #[test]
    fn canonicalize_replaces_local_option_ids_including_the_answer() {
        let question = canonicalize(multiple("q", "Enunciado", 2)).unwrap();
        assert_eq!(question.options[2].id, option_id("Gama"));
        assert_eq!(question.answer_id, option_id("Gama"));
        assert_eq!(question.answer_text(), Some("Gama"));
    }

    #[test]
    fn canonicalize_uses_the_content_fingerprint_as_the_question_id() {
        let question = canonicalize(multiple("q", "Enunciado", 2)).unwrap();
        assert_eq!(
            question.id,
            fingerprint(
                QuestionType::Multiple,
                "Enunciado",
                ["Alfa", "Beta", "Gama", "Delta", "Épsilon"],
                None
            )
        );
    }

    #[test]
    fn canonicalize_trims_topics() {
        let mut q = multiple("q", "Enunciado", 0);
        q.topics[0].area.insert(0, ' ');
        q.topics[0].matter.push(' ');
        assert_eq!(canonicalize(q).unwrap().topics, vec![topic()]);
    }

    #[test]
    fn canonicalize_deduplicates_topics_after_trimming() {
        let mut q = multiple("q", "Enunciado", 0);
        q.topics.push(TopicPath::new(
            " Direito",
            "Constitucional ",
            "Controle de constitucionalidade",
        ));
        assert_eq!(canonicalize(q).unwrap().topics, vec![topic()]);
    }

    #[test]
    fn canonicalize_deduplicates_references() {
        let mut q = multiple("q", "Enunciado", 0);
        q.refs = vec!["CF/88, art. 102".into(), "CF/88, art. 102".into()];
        assert_eq!(canonicalize(q).unwrap().refs, ["CF/88, art. 102"]);
    }

    #[test]
    fn canonicalize_reports_every_problem() {
        let mut q = multiple("q", "", 0);
        q.answer_id = "x".into();
        let problems = canonicalize(q).unwrap_err();
        assert!(problems.len() >= 2, "{problems:?}");
    }

    #[test]
    fn provenance_serializes_with_a_source_tag() {
        assert_eq!(
            serde_json::to_value(Provenance::Legacy).unwrap(),
            serde_json::json!({ "source": "legacy" })
        );
        assert_eq!(
            serde_json::to_value(Provenance::Generated {
                draft_id: "d1".into(),
                disputed: false,
            })
            .unwrap(),
            serde_json::json!({ "source": "generated", "draftId": "d1" })
        );
        assert_eq!(
            serde_json::to_value(Provenance::Generated {
                draft_id: "d1".into(),
                disputed: true,
            })
            .unwrap(),
            serde_json::json!({ "source": "generated", "draftId": "d1", "disputed": true })
        );
        // Older rows have no `disputed` key.
        let old: Provenance =
            serde_json::from_value(serde_json::json!({ "source": "generated", "draftId": "d1" }))
                .unwrap();
        assert_eq!(
            old,
            Provenance::Generated {
                draft_id: "d1".into(),
                disputed: false
            }
        );
    }
}
