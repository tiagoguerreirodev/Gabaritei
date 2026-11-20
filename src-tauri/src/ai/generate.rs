//! The generate role: a request becomes a validated draft package (spec §3.2 steps 1–2).

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
use crate::bank::store::{plan_import, topic_stems};
use crate::bank::{Package, QuestionType, TopicPath, package_json_schema};
use crate::db::Database;
use crate::error::{AppError, AppResult};

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

/// Problems with a draft's package, recomputed against the current bank each time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DraftCheck {
    /// pt-BR problems. Any problem blocks the import.
    pub problems: Vec<String>,
    /// How many questions are already in the bank (they merge instead of duplicating).
    pub duplicates: usize,
    pub multiple: usize,
    pub boolean: usize,
}

fn provenance(draft_id: &str) -> Provenance {
    Provenance::Generated {
        draft_id: draft_id.to_owned(),
    }
}

/// Validates a draft's package: structure, option rules, duplicates and the requested format.
pub fn check_draft(conn: &Connection, draft: &Draft) -> AppResult<DraftCheck> {
    let package = draft.package.clone();
    let inputs = package.into_inputs(&draft.request.request.topic, &provenance(&draft.id));
    let plan = plan_import(conn, inputs)?;
    let mut check = DraftCheck {
        problems: plan.problems.clone(),
        duplicates: plan.existing_count(),
        ..DraftCheck::default()
    };
    for question in &draft.package.questions {
        match question.kind {
            QuestionType::Multiple => check.multiple += 1,
            QuestionType::Boolean => check.boolean += 1,
        }
    }
    match draft.request.request.format {
        Some(QuestionType::Multiple) if check.boolean > 0 => check
            .problems
            .push("Você pediu só múltipla escolha, mas há questões Certo/Errado no pacote.".into()),
        Some(QuestionType::Boolean) if check.multiple > 0 => check.problems.push(
            "Você pediu só Certo/Errado, mas há questões de múltipla escolha no pacote.".into(),
        ),
        _ => {}
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
        id: draft::new_draft_id(),
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
