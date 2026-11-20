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

/// Number of options in a multiple-choice question.
pub const MULTIPLE_OPTIONS: usize = 5;
/// The two options of a Certo/Errado question.
pub const BOOLEAN_TEXTS: [&str; 2] = ["Certo", "Errado"];

pub const MAX_LOCAL_ID: usize = 200;
pub const MAX_STEM: usize = 100_000;
pub const MAX_OPTION_ID: usize = 100;
pub const MAX_OPTION_TEXT: usize = 20_000;
pub const MAX_EXPLANATION: usize = 100_000;
pub const MAX_REFS: usize = 50;
pub const MAX_REF: usize = 2_000;
pub const MIN_TOPICS: usize = 1;
pub const MAX_TOPICS: usize = 30;
pub const MAX_AREA: usize = 250;
pub const MAX_MATTER: usize = 250;
pub const MAX_TOPIC: usize = 500;
pub const MAX_IMAGE_ALT: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
/// `multiple`: five options, one correct. `boolean`: Certo/Errado.
pub enum QuestionType {
    Multiple,
    Boolean,
}

impl QuestionType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Multiple => "multiple",
            Self::Boolean => "boolean",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "multiple" => Some(Self::Multiple),
            "boolean" => Some(Self::Boolean),
            _ => None,
        }
    }
}

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

impl TopicPath {
    pub fn new(area: &str, matter: &str, topic: &str) -> Self {
        Self {
            area: area.to_owned(),
            matter: matter.to_owned(),
            topic: topic.to_owned(),
        }
    }

    /// The same path with surrounding whitespace removed from each name.
    fn trimmed(&self) -> Self {
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
    Generated { draft_id: String },
}

/// A question as received, before canonicalization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionInput {
    /// Identifier within its batch (package question ID or legacy ID), used in messages.
    pub local_id: String,
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

/// A canonical, stored question. `id` is the content fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub id: String,
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

impl Question {
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

/// `^[a-f0-9]{24,64}\.(png|jpg|jpeg|webp)$`
pub fn is_valid_image_name(name: &str) -> bool {
    let Some((stem, extension)) = name.split_once('.') else {
        return false;
    };
    (24..=64).contains(&stem.len())
        && stem.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        && matches!(extension, "png" | "jpg" | "jpeg" | "webp")
}

/// Every rule a question must follow. Returns pt-BR problems; empty means valid.
pub fn validate(input: &QuestionInput) -> Vec<String> {
    let mut problems = Vec::new();
    let mut problem = |message: String| problems.push(message);

    if is_blank(&input.local_id) || too_long(&input.local_id, MAX_LOCAL_ID) {
        problem(format!(
            "Identificador inválido (vazio ou com mais de {MAX_LOCAL_ID} caracteres)."
        ));
    }
    if is_blank(&input.stem) {
        problem("O enunciado está vazio.".into());
    } else if too_long(&input.stem, MAX_STEM) {
        problem(format!("O enunciado tem mais de {MAX_STEM} caracteres."));
    }

    match input.kind {
        QuestionType::Multiple => {
            if input.options.len() != MULTIPLE_OPTIONS {
                problem(format!(
                    "Questões de múltipla escolha têm {MULTIPLE_OPTIONS} alternativas; esta tem {}.",
                    input.options.len()
                ));
            }
        }
        QuestionType::Boolean => {
            let mut texts: Vec<&str> = input.options.iter().map(|o| o.text.as_str()).collect();
            texts.sort_unstable();
            if texts != BOOLEAN_TEXTS {
                problem(
                    "Questões Certo/Errado têm exatamente as alternativas \"Certo\" e \"Errado\"."
                        .into(),
                );
            }
        }
    }

    let mut ids = HashSet::new();
    let mut texts = HashSet::new();
    for option in &input.options {
        if is_blank(&option.id) || too_long(&option.id, MAX_OPTION_ID) {
            problem(format!(
                "Alternativa com identificador inválido (vazio ou com mais de {MAX_OPTION_ID} caracteres)."
            ));
        }
        if is_blank(&option.text) {
            problem("Há uma alternativa sem texto.".into());
        } else if too_long(&option.text, MAX_OPTION_TEXT) {
            problem(format!(
                "Uma alternativa tem mais de {MAX_OPTION_TEXT} caracteres."
            ));
        }
        if !ids.insert(option.id.as_str()) {
            problem(format!(
                "Identificador de alternativa repetido: {}.",
                option.id
            ));
        }
        if !texts.insert(js_trim(&option.text).to_lowercase()) {
            problem(format!(
                "Alternativas com o mesmo texto: \"{}\".",
                js_trim(&option.text)
            ));
        }
    }
    if !input.options.iter().any(|o| o.id == input.answer_id) {
        problem("O gabarito não corresponde a nenhuma alternativa.".into());
    }

    if too_long(&input.explanation, MAX_EXPLANATION) {
        problem(format!(
            "O comentário tem mais de {MAX_EXPLANATION} caracteres."
        ));
    }
    if input.refs.len() > MAX_REFS {
        problem(format!("Há mais de {MAX_REFS} referências."));
    }
    if input.refs.iter().any(|r| is_blank(r)) {
        problem("Há uma referência vazia.".into());
    }
    if input.refs.iter().any(|r| too_long(r, MAX_REF)) {
        problem(format!("Uma referência tem mais de {MAX_REF} caracteres."));
    }

    if !(MIN_TOPICS..=MAX_TOPICS).contains(&input.topics.len()) {
        problem(format!(
            "Cada questão precisa de {MIN_TOPICS} a {MAX_TOPICS} assuntos (área › matéria › assunto)."
        ));
    }
    for path in &input.topics {
        let names = [
            (&path.area, MAX_AREA),
            (&path.matter, MAX_MATTER),
            (&path.topic, MAX_TOPIC),
        ];
        if names.iter().any(|(name, _)| is_blank(name)) {
            problem("Área, matéria e assunto não podem ficar vazios.".into());
        }
        if names.iter().any(|(name, max)| too_long(name, *max)) {
            problem(format!(
                "Área, matéria ou assunto longo demais (limites: {MAX_AREA}, {MAX_MATTER} e {MAX_TOPIC} caracteres)."
            ));
        }
    }

    match (&input.image, &input.image_alt) {
        (Some(image), alt) => {
            if !is_valid_image_name(image) {
                problem(format!("Nome de imagem inválido: {image}."));
            }
            match alt {
                Some(alt) if !is_blank(alt) && !too_long(alt, MAX_IMAGE_ALT) => {}
                _ => problem(format!(
                    "A imagem precisa de uma descrição (texto alternativo) de até {MAX_IMAGE_ALT} caracteres."
                )),
            }
        }
        (None, Some(_)) => problem("Há texto alternativo, mas nenhuma imagem.".into()),
        (None, None) => {}
    }

    problems.dedup();
    problems
}

/// Validates the input and turns it into a canonical [`Question`]: option IDs derived from
/// their text, the answer key mapped accordingly, topic names trimmed and deduplicated,
/// and the ID set to the content fingerprint.
pub fn canonicalize(input: QuestionInput) -> Result<Question, Vec<String>> {
    let problems = validate(&input);
    if !problems.is_empty() {
        return Err(problems);
    }
    let answer_text = input
        .options
        .iter()
        .find(|o| o.id == input.answer_id)
        .map(|o| o.text.clone())
        .expect("validated: the answer matches an option");
    let options: Vec<QuestionOption> = input
        .options
        .into_iter()
        .map(|o| QuestionOption {
            id: option_id(&o.text),
            text: o.text,
        })
        .collect();
    let mut topics: Vec<TopicPath> = Vec::new();
    for path in input.topics.iter().map(TopicPath::trimmed) {
        if !topics.contains(&path) {
            topics.push(path);
        }
    }
    let mut refs: Vec<String> = Vec::new();
    for reference in input.refs {
        if !refs.contains(&reference) {
            refs.push(reference);
        }
    }
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
        answer_id: option_id(&answer_text),
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
            local_id: local_id.to_owned(),
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
            local_id: local_id.to_owned(),
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
        validate(&input)
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
    fn boolean_needs_exactly_certo_and_errado() {
        let mut q = boolean("q", "Enunciado", true);
        q.options[1].text = "Falso".into();
        assert_rejected(q, "\"Certo\" e \"Errado\"");
        let mut q = boolean("q", "Enunciado", true);
        q.options.push(QuestionOption {
            id: "x".into(),
            text: "Talvez".into(),
        });
        assert_rejected(q, "\"Certo\" e \"Errado\"");
    }

    #[test]
    fn option_ids_and_texts_must_be_distinct() {
        let mut q = multiple("q", "Enunciado", 0);
        q.options[1].id = "o0".into();
        assert_rejected(q, "Identificador de alternativa repetido");
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
    fn blank_and_oversized_texts_are_rejected() {
        assert_rejected(multiple("q", " \n ", 0), "O enunciado está vazio");
        assert_rejected(
            multiple("q", &"x".repeat(MAX_STEM + 1), 0),
            "O enunciado tem mais de",
        );
        let mut q = multiple("q", "Enunciado", 0);
        q.options[3].text = "\u{a0}".into();
        assert_rejected(q, "alternativa sem texto");
        let mut q = multiple("q", "Enunciado", 0);
        q.options[3].text = "y".repeat(MAX_OPTION_TEXT + 1);
        assert_rejected(q, "Uma alternativa tem mais de");
        let mut q = multiple("q", "Enunciado", 0);
        q.explanation = "z".repeat(MAX_EXPLANATION + 1);
        assert_rejected(q, "O comentário tem mais de");
        assert_rejected(multiple("", "Enunciado", 0), "Identificador inválido");
    }

    #[test]
    fn refs_are_bounded() {
        let mut q = multiple("q", "Enunciado", 0);
        q.refs = vec!["Lei 8.112/1990".into(); MAX_REFS + 1];
        assert_rejected(q, "Há mais de 50 referências");
        let mut q = multiple("q", "Enunciado", 0);
        q.refs = vec![" ".into()];
        assert_rejected(q, "referência vazia");
        let mut q = multiple("q", "Enunciado", 0);
        q.refs = vec!["r".repeat(MAX_REF + 1)];
        assert_rejected(q, "Uma referência tem mais de");
    }

    #[test]
    fn topics_need_one_to_thirty_complete_paths() {
        let mut q = multiple("q", "Enunciado", 0);
        q.topics.clear();
        assert_rejected(q, "de 1 a 30 assuntos");
        let mut q = multiple("q", "Enunciado", 0);
        q.topics = (0..31)
            .map(|i| TopicPath::new("A", "M", &format!("T{i}")))
            .collect();
        assert_rejected(q, "de 1 a 30 assuntos");
        let mut q = multiple("q", "Enunciado", 0);
        q.topics = vec![TopicPath::new("Direito", " ", "Crase")];
        assert_rejected(q, "não podem ficar vazios");
        let mut q = multiple("q", "Enunciado", 0);
        q.topics = vec![TopicPath::new(&"a".repeat(MAX_AREA + 1), "M", "T")];
        assert_rejected(q, "longo demais");
    }

    #[test]
    fn images_need_a_valid_name_and_alt_text() {
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

        let mut q = multiple("q", "Enunciado", 0);
        q.image = Some("../segredo.png".into());
        q.image_alt = Some("Figura".into());
        assert_rejected(q, "Nome de imagem inválido");
        let mut q = multiple("q", "Enunciado", 0);
        q.image = Some("0123456789abcdef0123456789abcdef.png".into());
        assert_rejected(q, "precisa de uma descrição");
        let mut q = multiple("q", "Enunciado", 0);
        q.image_alt = Some("Figura".into());
        assert_rejected(q, "nenhuma imagem");
    }

    #[test]
    fn canonicalize_derives_option_ids_answer_and_fingerprint() {
        let question = canonicalize(multiple("q", "Enunciado", 2)).unwrap();
        assert_eq!(question.options[2].id, option_id("Gama"));
        assert_eq!(question.answer_id, option_id("Gama"));
        assert_eq!(question.answer_text(), Some("Gama"));
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
    fn canonicalize_trims_topics_and_drops_duplicate_topics_and_refs() {
        let mut q = multiple("q", "Enunciado", 0);
        q.topics = vec![
            topic(),
            TopicPath::new(
                " Direito",
                "Constitucional ",
                "Controle de constitucionalidade",
            ),
        ];
        q.refs = vec!["CF/88, art. 102".into(), "CF/88, art. 102".into()];
        let question = canonicalize(q).unwrap();
        assert_eq!(question.topics, vec![topic()]);
        assert_eq!(question.refs, vec!["CF/88, art. 102".to_owned()]);
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
                draft_id: "d1".into()
            })
            .unwrap(),
            serde_json::json!({ "source": "generated", "draftId": "d1" })
        );
    }
}
