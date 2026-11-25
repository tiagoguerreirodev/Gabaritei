//! The generate role: a request becomes a validated draft package (spec §3.2 steps 1–2).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::draft::{self, Draft, DraftRequest, DraftStatus, GenerationInfo};
use super::prompts;
use super::roles::RoleConfig;
use super::runner::{CancelToken, Progress, run_job};
use super::secrets::Secrets;
use super::settings::{self, RemNoteMode, ResolvedClis, Sources};
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
        self.validate_topic()?;
        self.validate_text_fields()?;
        self.validate_roles()?;
        self.validate_attachments()
    }

    fn validate_topic(&mut self) -> AppResult<()> {
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
        Ok(())
    }

    fn validate_text_fields(&mut self) -> AppResult<()> {
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
        Ok(())
    }

    fn validate_roles(&mut self) -> AppResult<()> {
        for (config, label) in [
            (self.generator.as_mut(), "Gerador"),
            (self.checker.as_mut(), "Conferente"),
        ] {
            if let Some(config) = config {
                super::settings::validate_role_config(config, label)?;
            }
        }
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

/// Only fixed messages can leave the backend during draft preview.
fn preview_message(problem: &crate::bank::question::QuestionProblem) -> &'static str {
    use crate::bank::question::QuestionProblem::*;
    match problem {
        InvalidId => "O identificador da questão é inválido.",
        EmptyStem | LongStem => "O enunciado está vazio ou é longo demais.",
        MultipleOptionCount(_) => {
            "Questões de múltipla escolha precisam ter exatamente 5 alternativas."
        }
        BooleanOptions => {
            "Questões Certo/Errado precisam ter exatamente as alternativas Certo e Errado."
        }
        InvalidOptionId => "Há uma alternativa com identificador inválido.",
        RepeatedOptionId(_) => "Há alternativas com o mesmo identificador.",
        EmptyOptionText => "Há uma alternativa sem texto.",
        LongOptionText => "Há uma alternativa longa demais.",
        RepeatedOptionText(_) => "Há alternativas com o mesmo texto.",
        InvalidAnswer => "O gabarito não corresponde a nenhuma alternativa.",
        LongExplanation => "O comentário é longo demais.",
        TooManyRefs => "Há referências demais.",
        EmptyRef => "Há uma referência vazia.",
        LongRef => "Há uma referência longa demais.",
        TopicCount => "A questão precisa de um assunto válido.",
        EmptyTopicName | LongTopicName => "Área, matéria ou assunto vazio ou longo demais.",
        ImageName(_) | ImageDescription | DescriptionWithoutImage => {
            "A questão não passou na validação."
        }
    }
}

fn provenance(draft_id: &DraftId, disputed: bool) -> Provenance {
    Provenance::Generated {
        draft_id: draft_id.to_owned(),
        disputed,
    }
}

/// The package as question inputs for the draft's topic, in package order.
pub fn draft_inputs(draft: &Draft, disputed: &[usize]) -> Vec<crate::bank::QuestionInput> {
    draft
        .package
        .clone()
        .into_inputs(&draft.request.request.topic, &provenance(&draft.id, false))
        .into_iter()
        .enumerate()
        .map(|(index, mut input)| {
            input.provenance = provenance(&draft.id, disputed.contains(&index));
            input
        })
        .collect()
}

/// Validates a draft's package: structure, option rules, duplicates and the requested format.
pub fn check_draft(conn: &Connection, draft: &Draft) -> AppResult<DraftCheck> {
    let mut check = DraftCheck::default();
    let mut seen: HashMap<QuestionId, (u32, String)> = HashMap::new();
    let mut local_ids: HashSet<&str> = HashSet::new();
    let format = draft.request.request.format;
    let inputs = draft_inputs(draft, &[]);

    for (index, (input, question)) in inputs.into_iter().zip(&draft.package.questions).enumerate() {
        let position = u32::try_from(index + 1).unwrap_or(u32::MAX);
        check.count_format(position, format, question.kind);
        if !local_ids.insert(question.id.as_str()) {
            check.problem(
                position,
                "O identificador desta questão repete o de outra do pacote.".into(),
            );
            continue;
        }
        match canonicalize(input) {
            Ok(canonical) => check.check_duplicates(conn, &mut seen, position, canonical)?,
            Err(problems) => {
                for problem in problems {
                    check.problem(position, preview_message(&problem).into());
                }
            }
        }
    }
    Ok(check)
}

impl DraftCheck {
    fn problem(&mut self, position: u32, message: String) {
        self.problems.push(DraftProblem {
            position: Some(position),
            message,
        });
    }

    fn count_format(&mut self, position: u32, format: Option<QuestionType>, kind: QuestionType) {
        match kind {
            QuestionType::Multiple => self.multiple += 1,
            QuestionType::Boolean => self.boolean += 1,
        }
        match (format, kind) {
            (Some(QuestionType::Multiple), QuestionType::Boolean) => {
                self.problem(
                    position,
                    "Você pediu só múltipla escolha, mas esta é Certo/Errado.".into(),
                );
            }
            (Some(QuestionType::Boolean), QuestionType::Multiple) => {
                self.problem(
                    position,
                    "Você pediu só Certo/Errado, mas esta é de múltipla escolha.".into(),
                );
            }
            (Some(QuestionType::Multiple), QuestionType::Multiple)
            | (Some(QuestionType::Boolean), QuestionType::Boolean)
            | (None, QuestionType::Multiple | QuestionType::Boolean) => {}
        }
    }

    fn check_duplicates(
        &mut self,
        conn: &Connection,
        seen: &mut HashMap<QuestionId, (u32, String)>,
        position: u32,
        canonical: crate::bank::Question,
    ) -> AppResult<()> {
        match seen.get(&canonical.id) {
            Some((first, answer)) if *answer != canonical.answer_id => {
                self.problem(
                    position,
                    format!("Repete a Q{first}, mas com resposta divergente."),
                );
            }
            Some(_) => self.duplicates += 1,
            None => {
                seen.insert(
                    canonical.id.clone(),
                    (position, canonical.answer_id.clone()),
                );
            }
        }
        if let Some(stored) = load_question(conn, &canonical.id)? {
            if stored.answer_id == canonical.answer_id {
                self.duplicates += 1;
            } else {
                self.problem(
                    position,
                    "Já existe no banco com resposta divergente.".into(),
                );
            }
        }
        Ok(())
    }
}

/// Executables and clocks supplied by the command for a generation or retry.
pub struct GenerationRuntime<'a> {
    pub clis: &'a ResolvedClis,
    pub now: &'a dyn Fn() -> String,
    pub monotonic_time: &'a dyn Fn() -> Duration,
}

/// Runs the generator and saves the result as a draft. Blocking: call from a worker thread.
///
/// The database is only locked briefly before and after the run, never during it.
pub fn generate_draft(
    db: &Database,
    secrets: &Secrets,
    runtime: &GenerationRuntime<'_>,
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

    let spec = settings::job_spec(
        &settings,
        secrets,
        runtime.clis,
        &generator,
        package_json_schema(),
        Sources {
            remnote: request.sources.remnote,
            web: request.sources.web,
        },
        request
            .sources
            .attachments
            .iter()
            .map(PathBuf::from)
            .collect(),
    )?
    .with_input(
        prompts::generate_instructions(),
        prompts::generate_prompt(&request, &stems),
    );

    let started_at = (runtime.now)();
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
        created_at: (runtime.now)(),
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
