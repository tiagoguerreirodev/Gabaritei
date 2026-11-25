//! The generate role: a request becomes a validated draft package (spec §3.2 steps 1–2).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::draft::{self, Draft, DraftRequest, DraftStatus, GenerationInfo};
use super::prompts;
use super::roles::RoleConfig;
use super::runner::{CancelToken, Progress, run_job};
use super::secrets::Secrets;
use super::settings::{self, RemNoteMode, Sources};
use crate::bank::question::{MAX_AREA, MAX_MATTER, MAX_TOPIC, Provenance};
use crate::bank::store::{load_question, topic_stems};
use crate::bank::{Package, QuestionType, TopicPath, canonicalize, package_json_schema};
use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::ids::{self, DraftId, QuestionId};

pub const MAX_COUNT: u32 = 30;
const MAX_BOARD: usize = 100;
const MAX_INSTRUCTIONS: usize = 4_000;
const MAX_ATTACHMENTS: usize = 5;
const MAX_ATTACHMENT_BYTES: u64 = 20 * 1024 * 1024;
const ATTACHMENT_EXTENSIONS: &[&str] = &["pdf", "md", "txt"];

/// What a generation may read besides the model's own knowledge.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GenerateSources {
    pub remnote: bool,
    pub web: bool,
    /// Paths of PDF, Markdown or text files. Stored in the draft by file name only.
    pub attachments: Vec<String>,
}

/// The generation form (spec §3.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GenerateRequest {
    /// An existing topic or a new path.
    pub topic: TopicPath,
    /// 1 to 30 questions.
    pub count: u32,
    /// Board style, e.g. "Cebraspe". Optional.
    pub board: Option<String>,
    /// `None` lets the generator mix both formats.
    pub format: Option<QuestionType>,
    pub instructions: Option<String>,
    pub sources: GenerateSources,
    /// Overrides of the role defaults for this generation only.
    pub generator: Option<RoleConfig>,
    pub checker: Option<RoleConfig>,
}

fn blank(text: &str) -> bool {
    text.trim().is_empty()
}

impl GenerateRequest {
    /// Checks the form and tidies it (trimmed names, blank optionals removed).
    pub fn validate(&mut self) -> AppResult<()> {
        if !(1..=MAX_COUNT).contains(&self.count) {
            return Err(AppError::Invalid(format!(
                "Escolha de 1 a {MAX_COUNT} questões."
            )));
        }
        for (value, label, max) in [
            (&mut self.topic.area, "A área", MAX_AREA),
            (&mut self.topic.matter, "A matéria", MAX_MATTER),
            (&mut self.topic.topic, "O assunto", MAX_TOPIC),
        ] {
            *value = value.trim().to_owned();
            if value.is_empty() {
                return Err(AppError::Invalid(format!("{label} não pode ficar vazio.")));
            }
            if value.chars().count() > max {
                return Err(AppError::Invalid(format!(
                    "{label} tem mais de {max} caracteres."
                )));
            }
        }
        self.board = self
            .board
            .take()
            .filter(|b| !blank(b))
            .map(|b| b.trim().to_owned());
        if self
            .board
            .as_ref()
            .is_some_and(|b| b.chars().count() > MAX_BOARD)
        {
            return Err(AppError::Invalid(format!(
                "O nome da banca tem mais de {MAX_BOARD} caracteres."
            )));
        }
        self.instructions = self
            .instructions
            .take()
            .filter(|i| !blank(i))
            .map(|i| i.trim().to_owned());
        if self
            .instructions
            .as_ref()
            .is_some_and(|i| i.chars().count() > MAX_INSTRUCTIONS)
        {
            return Err(AppError::Invalid(format!(
                "As instruções têm mais de {MAX_INSTRUCTIONS} caracteres."
            )));
        }
        for (config, label) in [
            (self.generator.as_mut(), "Gerador"),
            (self.checker.as_mut(), "Conferente"),
        ] {
            if let Some(config) = config {
                super::settings::validate_role_config(config, label)?;
            }
        }
        self.validate_attachments()?;
        Ok(())
    }

    fn validate_attachments(&self) -> AppResult<()> {
        let files = &self.sources.attachments;
        if files.len() > MAX_ATTACHMENTS {
            return Err(AppError::Invalid(format!(
                "Anexe no máximo {MAX_ATTACHMENTS} arquivos."
            )));
        }
        for file in files {
            let path = Path::new(file);
            let name = path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            let extension = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if !ATTACHMENT_EXTENSIONS.contains(&extension.as_str()) {
                return Err(AppError::Invalid(format!(
                    "O anexo “{name}” não é um PDF, Markdown ou texto."
                )));
            }
            let meta = std::fs::metadata(path)
                .ok()
                .filter(std::fs::Metadata::is_file)
                .ok_or_else(|| AppError::Invalid(format!("Não encontrei o anexo “{name}”.")))?;
            if meta.len() > MAX_ATTACHMENT_BYTES {
                return Err(AppError::Invalid(format!(
                    "O anexo “{name}” passa de {} MB.",
                    MAX_ATTACHMENT_BYTES / 1024 / 1024
                )));
            }
        }
        Ok(())
    }

    /// The request as kept in the draft: attachments by file name, not by path.
    fn for_storage(&self) -> Self {
        let mut stored = self.clone();
        stored.sources.attachments = self
            .sources
            .attachments
            .iter()
            .map(|file| {
                Path::new(file)
                    .file_name()
                    .map_or_else(|| file.clone(), |n| n.to_string_lossy().into_owned())
            })
            .collect();
        stored
    }
}

/// One problem found in a draft. The message never quotes question text, so it is safe to show
/// in the spoiler-free preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DraftProblem {
    /// 1-based position of the question, or `None` for a problem with the whole package.
    pub position: Option<u32>,
    /// pt-BR.
    pub message: String,
}

/// Problems with a draft's package, recomputed against the current bank each time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DraftCheck {
    pub problems: Vec<DraftProblem>,
    /// How many questions are already in the bank, or repeat another question of the package
    /// (they merge instead of duplicating).
    pub duplicates: usize,
    pub multiple: usize,
    pub boolean: usize,
}

impl DraftCheck {
    /// Whether the question at 0-based `index` has a problem of its own.
    pub fn is_blocked(&self, index: usize) -> bool {
        self.problems
            .iter()
            .any(|p| p.position == u32::try_from(index + 1).ok())
    }
}

/// A fixed pt-BR message for each rule `validate` can report. The validator's own messages
/// quote option texts, option IDs and counts chosen by the AI, so they are never passed on:
/// anything the table does not know becomes a generic message. Matching is by the rule's fixed
/// prefix, never by anything the question contains.
pub fn fixed_message(problem: &str) -> &'static str {
    const RULES: &[(&str, &str)] = &[
        (
            "Identificador inválido",
            "O identificador da questão é inválido.",
        ),
        ("O enunciado", "O enunciado está vazio ou é longo demais."),
        (
            "Questões de múltipla escolha têm",
            "Questões de múltipla escolha precisam ter exatamente 5 alternativas.",
        ),
        (
            "Questões Certo/Errado têm",
            "Questões Certo/Errado precisam ter exatamente as alternativas Certo e Errado.",
        ),
        (
            "Alternativa com identificador inválido",
            "Há uma alternativa com identificador inválido.",
        ),
        (
            "Identificador de alternativa repetido",
            "Há alternativas com o mesmo identificador.",
        ),
        (
            "Há uma alternativa sem texto",
            "Há uma alternativa sem texto.",
        ),
        (
            "Uma alternativa tem mais de",
            "Há uma alternativa longa demais.",
        ),
        (
            "Alternativas com o mesmo texto",
            "Há alternativas com o mesmo texto.",
        ),
        (
            "O gabarito não corresponde",
            "O gabarito não corresponde a nenhuma alternativa.",
        ),
        ("O comentário tem mais de", "O comentário é longo demais."),
        ("Há mais de", "Há referências demais."),
        ("Há uma referência vazia", "Há uma referência vazia."),
        (
            "Uma referência tem mais de",
            "Há uma referência longa demais.",
        ),
        (
            "Cada questão precisa de",
            "A questão precisa de um assunto válido.",
        ),
        (
            "Área, matéria",
            "Área, matéria ou assunto vazio ou longo demais.",
        ),
    ];
    RULES
        .iter()
        .find(|(prefix, _)| problem.starts_with(prefix))
        .map_or("A questão não passou na validação.", |(_, message)| {
            message
        })
}

fn provenance(draft_id: &DraftId, disputed: bool) -> Provenance {
    Provenance::Generated {
        draft_id: draft_id.to_owned(),
        disputed,
    }
}

/// The package as question inputs for the draft's topic, in package order.
pub fn draft_inputs(
    draft: &Draft,
    disputed: &dyn Fn(usize) -> bool,
) -> Vec<crate::bank::QuestionInput> {
    draft
        .package
        .clone()
        .into_inputs(&draft.request.request.topic, &provenance(&draft.id, false))
        .into_iter()
        .enumerate()
        .map(|(index, mut input)| {
            input.provenance = provenance(&draft.id, disputed(index));
            input
        })
        .collect()
}

/// Validates a draft's package: structure, option rules, duplicates and the requested format.
pub fn check_draft(conn: &Connection, draft: &Draft) -> AppResult<DraftCheck> {
    let mut check = DraftCheck::default();
    let mut seen: HashMap<QuestionId, (usize, String)> = HashMap::new();
    let mut local_ids: HashSet<&str> = HashSet::new();
    let format = draft.request.request.format;
    let inputs = draft_inputs(draft, &|_| false);

    for (index, (input, question)) in inputs.into_iter().zip(&draft.package.questions).enumerate() {
        let position = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let mut problem = |message: String| {
            check.problems.push(DraftProblem {
                position: Some(position),
                message,
            });
        };
        match question.kind {
            QuestionType::Multiple => check.multiple += 1,
            QuestionType::Boolean => check.boolean += 1,
        }
        match (format, question.kind) {
            (Some(QuestionType::Multiple), QuestionType::Boolean) => {
                problem("Você pediu só múltipla escolha, mas esta é Certo/Errado.".into());
            }
            (Some(QuestionType::Boolean), QuestionType::Multiple) => {
                problem("Você pediu só Certo/Errado, mas esta é de múltipla escolha.".into());
            }
            (Some(QuestionType::Multiple), QuestionType::Multiple)
            | (Some(QuestionType::Boolean), QuestionType::Boolean)
            | (None, QuestionType::Multiple | QuestionType::Boolean) => {}
        }
        if !local_ids.insert(question.id.as_str()) {
            problem("O identificador desta questão repete o de outra do pacote.".into());
            continue;
        }
        match canonicalize(input) {
            Err(problems) => {
                for message in problems {
                    problem(fixed_message(&message).to_owned());
                }
            }
            Ok(canonical) => {
                match seen.get(&canonical.id) {
                    Some((first, answer)) if *answer != canonical.answer_id => {
                        problem(format!("Repete a Q{first}, mas com resposta divergente."));
                    }
                    Some(_) => check.duplicates += 1,
                    None => {
                        seen.insert(
                            canonical.id.clone(),
                            (index + 1, canonical.answer_id.clone()),
                        );
                    }
                }
                if let Some(stored) = load_question(conn, &canonical.id)? {
                    if stored.answer_id == canonical.answer_id {
                        check.duplicates += 1;
                    } else {
                        problem("Já existe no banco com resposta divergente.".into());
                    }
                }
            }
        }
    }
    Ok(check)
}

/// Runs the generator and saves the result as a draft. Blocking: call from a worker thread.
///
/// The database is only locked briefly before and after the run, never during it.
pub fn generate_draft(
    db: &Database,
    secrets: &Secrets,
    mut request: GenerateRequest,
    cancel: &CancelToken,
    on_progress: &mut dyn FnMut(Progress),
) -> AppResult<Draft> {
    request.validate()?;
    let (settings, stems) = db.with(|conn| {
        Ok((
            settings::load(conn, secrets)?,
            topic_stems(conn, &request.topic)?,
        ))
    })?;
    if request.sources.remnote && settings.remnote.mode == RemNoteMode::Off {
        return Err(AppError::Invalid(
            "O RemNote está desligado. Ligue-o em Configurações ou desmarque a fonte.".into(),
        ));
    }
    let generator = request
        .generator
        .clone()
        .unwrap_or_else(|| settings.roles.generate.clone());
    let checker = request
        .checker
        .clone()
        .unwrap_or_else(|| settings.roles.blind_check.clone());

    let mut spec = settings::job_spec(
        &settings,
        secrets,
        &generator,
        package_json_schema(),
        Sources {
            remnote: request.sources.remnote,
            web: request.sources.web,
        },
    )?;
    spec.instructions = prompts::generate_instructions();
    spec.prompt = prompts::generate_prompt(&request, &stems);
    spec.attachments = request
        .sources
        .attachments
        .iter()
        .map(PathBuf::from)
        .collect();

    let started_at = crate::clock::now();
    let output = run_job(&spec, cancel, on_progress)?;
    let package: Package = output.parse()?;
    if package.questions.len() != request.count as usize {
        return Err(AppError::Invalid(format!(
            "A IA devolveu {} questões, mas você pediu {}. Tente de novo.",
            package.questions.len(),
            request.count
        )));
    }

    let draft = Draft {
        id: ids::generate::<DraftId>(&mut rand::rng()),
        created_at: crate::clock::now(),
        status: DraftStatus::Review,
        request: DraftRequest {
            request: request.for_storage(),
            info: GenerationInfo {
                agent: generator.agent,
                model: generator.model.clone(),
                effort: generator.effort,
                started_at,
                duration_ms: output.duration_ms,
                cost_usd: output.cost_usd,
            },
            generator,
            checker,
        },
        package,
        // The blind check starts right after; until it ends the draft cannot be imported.
        blind_check: Some(super::blind_check::pending_record()),
    };
    db.with(|conn| draft::insert(conn, &draft))?;
    Ok(draft)
}

#[cfg(test)]
mod tests;
