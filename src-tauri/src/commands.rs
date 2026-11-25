//! Tauri commands: thin wrappers that call domain functions.
//!
//! Every command must also be added to [`crate::register`] and wrapped in `src/api.ts`.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_dialog::DialogExt;
use ts_rs::TS;

use crate::ai::agent::Agent;
use crate::ai::blind_check;
use crate::ai::catalog::{self as model_catalog, CatalogCache};
use crate::ai::discovery::{self, CliStatus};
use crate::ai::draft_files;
use crate::ai::explain;
use crate::ai::generate::GenerateRequest;
use crate::ai::jobs::{GenerationKind, GenerationManager, GenerationStatus};
use crate::ai::ondemand::OnDemand;
use crate::ai::recheck::{self, RecheckView};
use crate::ai::review::{self, DraftListItem, DraftSummary, ImportMode, ImportResult};
use crate::ai::roles::RoleSettings;
use crate::ai::secrets::Secrets;
use crate::ai::settings::{self, AiSettings, AiSettingsInput, RemNoteTest};
use crate::appearance::{self, Appearance};
use crate::attempts::{self, AttemptView, SessionStart};
use crate::backup::{BackupInfo, BackupService, BackupStatus, OpenableFolder, RelocateOutcome};
use crate::bank::catalog::{self, CollectionDetail, CollectionPage, CollectionQuery, TopicTree};
use crate::db::Database;
use crate::error::{AppError, AppResult, ErrorPayload};
use crate::ids::{AttemptId, CollectionId, DraftId};
use crate::learning::{self, LearningOverview};
use crate::metrics::{self, HomeSummary, Performance, PerformanceQuery};
use crate::migrate::{self, LegacyImportOutcome, LegacyImportStatus};
use crate::paths::AppPaths;
use crate::shortcuts;

/// Tauri calls this conversion once when rejecting an IPC command.
impl From<AppError> for tauri::ipc::InvokeError {
    fn from(error: AppError) -> Self {
        if let AppError::Internal(detail) = &error {
            crate::log::error(format!("unexpected error: {detail}"));
        }
        ErrorPayload::from(&error).into()
    }
}

/// Basic information about the running app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

/// Returns the app's name and version (from `Cargo.toml`).
pub fn current_app_info() -> AppInfo {
    AppInfo {
        name: "Gabaritei".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    }
}

#[tauri::command]
pub fn app_info() -> AppResult<AppInfo> {
    Ok(current_app_info())
}

/// The saved colour theme and text size.
#[tauri::command]
pub fn appearance(db: State<'_, Database>) -> AppResult<Appearance> {
    db.with(|conn| appearance::load(conn))
}

/// Saves the colour theme and text size, and returns them.
#[tauri::command]
pub fn save_appearance(db: State<'_, Database>, appearance: Appearance) -> AppResult<Appearance> {
    db.with(|conn| appearance::save(conn, appearance))
}

/// Whether the Resolver's single-letter shortcuts are on.
#[tauri::command]
pub fn keyboard_shortcuts(db: State<'_, Database>) -> AppResult<bool> {
    db.with(|conn| shortcuts::load(conn))
}

/// Turns the Resolver's single-letter shortcuts on or off; returns the saved value.
#[tauri::command]
pub fn save_keyboard_shortcuts(db: State<'_, Database>, enabled: bool) -> AppResult<bool> {
    db.with(|conn| shortcuts::save(conn, enabled))
}

/// Area › matter › topic with question counts.
#[tauri::command(async)]
pub fn topics_tree(db: State<'_, Database>) -> AppResult<TopicTree> {
    db.with(|conn| catalog::topic_tree(conn))
}

/// One page of collections matching the search and filters.
#[tauri::command]
pub fn collections(db: State<'_, Database>, query: CollectionQuery) -> AppResult<CollectionPage> {
    db.with(|conn| catalog::collections(conn, &query))
}

/// One collection with its attempts.
#[tauri::command]
pub fn collection(db: State<'_, Database>, id: CollectionId) -> AppResult<CollectionDetail> {
    db.with(|conn| catalog::collection(conn, &id))
}

/// Whether the legacy bank was imported, and whether the bank is still empty.
#[tauri::command]
pub fn legacy_import_status(db: State<'_, Database>) -> AppResult<LegacyImportStatus> {
    migrate::status(&db)
}

/// Asks for the legacy repository folder and imports its bank. `None` if the user cancels.
#[tauri::command]
pub async fn import_legacy_bank<R: Runtime>(
    app: AppHandle<R>,
) -> AppResult<Option<LegacyImportOutcome>> {
    let Some(folder) = app
        .dialog()
        .file()
        .set_title("Escolha a pasta do Caderno Local (repositório Simulados-Concursos)")
        .blocking_pick_folder()
    else {
        return Ok(None);
    };
    let source = folder
        .into_path()
        .map_err(|_| AppError::Invalid("Escolha uma pasta deste computador.".into()))?;
    tauri::async_runtime::spawn_blocking(move || {
        let db = app.state::<Database>();
        let paths = app.state::<AppPaths>();
        let service = app.state::<BackupService>();
        // Not while a backup, restore or data-file import runs: they must never overlap. The
        // frontend's busy flag is only a convenience; this is what enforces it.
        service.exclusive(|| {
            migrate::import_legacy_bank(&db, &paths.images_dir(), &source, &crate::clock::now())
                .map(Some)
        })
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

/// Where backups go, the ones that exist and the last backup failure.
#[tauri::command(async)]
pub fn backup_status(
    db: State<'_, Database>,
    paths: State<'_, AppPaths>,
    service: State<'_, BackupService>,
) -> AppResult<BackupStatus> {
    service.status(&db, &paths)
}

/// The app data folder, shown in Configurações → Dados.
#[tauri::command]
pub fn data_folder(paths: State<'_, AppPaths>) -> String {
    paths.data_dir().display().to_string()
}

/// Opens the app data folder or the backup folder in the system file manager. The frontend
/// can only name one of the two; it never sends a path.
#[tauri::command(async)]
pub fn open_folder<R: Runtime>(
    app: AppHandle<R>,
    db: State<'_, Database>,
    paths: State<'_, AppPaths>,
    which: OpenableFolder,
) -> AppResult<()> {
    let path = crate::backup::folder_to_open(&db, &paths, which)?;
    tauri_plugin_opener::OpenerExt::opener(&app)
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|error| {
            crate::log::error(format!("could not open the folder: {error}"));
            AppError::Invalid("Não foi possível abrir a pasta.".into())
        })
}

/// Makes a backup now.
#[tauri::command(async)]
pub fn backup_now(
    db: State<'_, Database>,
    paths: State<'_, AppPaths>,
    service: State<'_, BackupService>,
) -> AppResult<BackupInfo> {
    service.backup_now(&db, &paths, &crate::clock::now())
}

/// Asks for a new backup folder and saves it. `None` if the user cancels.
#[tauri::command]
pub async fn backup_choose_folder<R: Runtime>(
    app: AppHandle<R>,
) -> AppResult<Option<BackupStatus>> {
    let Some(folder) = app
        .dialog()
        .file()
        .set_title("Escolha a pasta dos backups")
        .blocking_pick_folder()
    else {
        return Ok(None);
    };
    let chosen = folder
        .into_path()
        .map_err(|_| AppError::Invalid("Escolha uma pasta deste computador.".into()))?;
    tauri::async_runtime::spawn_blocking(move || {
        let db = app.state::<Database>();
        let paths = app.state::<AppPaths>();
        let service = app.state::<BackupService>();
        service.change_folder(&db, &paths, Some(&chosen)).map(Some)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

/// Goes back to the default backup folder.
#[tauri::command(async)]
pub fn backup_use_default_folder(
    db: State<'_, Database>,
    paths: State<'_, AppPaths>,
    service: State<'_, BackupService>,
) -> AppResult<BackupStatus> {
    service.change_folder(&db, &paths, None)
}

/// Copies (or, with `remove_originals`, moves) the backups left in the old folder by the last
/// folder change to the current one. Each original is deleted only after its copy is verified.
#[tauri::command(async)]
pub fn backup_relocate_left_behind(
    db: State<'_, Database>,
    paths: State<'_, AppPaths>,
    service: State<'_, BackupService>,
    remove_originals: bool,
) -> AppResult<RelocateOutcome> {
    service.relocate_left_behind(&db, &paths, remove_originals)
}

/// Forgets the offer to move backups from the old folder (the "Entendi" button). Nothing is
/// moved or deleted.
#[tauri::command(async)]
pub fn backup_forget_left_behind(
    db: State<'_, Database>,
    service: State<'_, BackupService>,
) -> AppResult<()> {
    service.forget_left_behind(&db)
}

/// Replaces the current data with a backup, after a safety backup. Not while a generation runs.
#[tauri::command(async)]
pub fn backup_restore(
    db: State<'_, Database>,
    paths: State<'_, AppPaths>,
    service: State<'_, BackupService>,
    generation: State<'_, GenerationManager>,
    on_demand: State<'_, OnDemand>,
    name: String,
) -> AppResult<()> {
    service.restore(&db, &paths, &name, &crate::clock::now(), || {
        ensure_no_ai_job(&generation, &on_demand, "restaurar")
    })?;
    on_demand.clear_proposals();
    Ok(())
}

/// Replacing the data under a running AI job would lose what the job writes.
fn ensure_no_ai_job(
    generation: &GenerationManager,
    on_demand: &OnDemand,
    action: &str,
) -> AppResult<()> {
    if generation.status().running || on_demand.is_running() {
        return Err(AppError::Invalid(format!(
            "Há uma consulta à IA em andamento. Espere ou cancele antes de {action}."
        )));
    }
    Ok(())
}

/// Backups, restores and imports replace the data, so no AI job starts meanwhile.
fn ensure_not_backing_up(service: &BackupService) -> AppResult<()> {
    if service.is_busy() {
        return Err(AppError::Invalid(
            "Um backup, restauração, importação ou a cópia de backups para a pasta nova está em andamento. Tente de novo em instantes."
                .into(),
        ));
    }
    Ok(())
}

/// Asks where to save the data file and writes it. `false` if the user cancels.
#[tauri::command]
pub async fn export_data<R: Runtime>(app: AppHandle<R>) -> AppResult<bool> {
    tauri::async_runtime::spawn_blocking(move || {
        let now = crate::clock::now();
        let Some(file) = app
            .dialog()
            .file()
            .set_title("Exportar dados")
            .set_file_name(crate::backup::datafile::file_name(&now))
            .add_filter("Dados do Gabaritei", &[crate::backup::datafile::EXTENSION])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let dest = file
            .into_path()
            .map_err(|_| AppError::Invalid("Escolha um local deste computador.".into()))?;
        let db = app.state::<Database>();
        let paths = app.state::<AppPaths>();
        let service = app.state::<BackupService>();
        service.export(&db, &paths, &dest, &now).map(|()| true)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

/// Asks for a data file and replaces the local data with it, after a safety backup. `false` if
/// the user cancels. Not while a generation runs.
#[tauri::command]
pub async fn import_data<R: Runtime>(app: AppHandle<R>) -> AppResult<bool> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(file) = app
            .dialog()
            .file()
            .set_title("Importar dados")
            .add_filter("Dados do Gabaritei", &[crate::backup::datafile::EXTENSION])
            .blocking_pick_file()
        else {
            return Ok(false);
        };
        let source = file
            .into_path()
            .map_err(|_| AppError::Invalid("Escolha um arquivo deste computador.".into()))?;
        let db = app.state::<Database>();
        let paths = app.state::<AppPaths>();
        let service = app.state::<BackupService>();
        let generation = app.state::<GenerationManager>();
        let on_demand = app.state::<OnDemand>();
        service.import(&db, &paths, &source, &crate::clock::now(), || {
            ensure_no_ai_job(&generation, &on_demand, "importar")
        })?;
        on_demand.clear_proposals();
        Ok(true)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

/// Topic strength, weekly trend and filter options under the given filters.
#[tauri::command(async)]
pub fn performance(db: State<'_, Database>, query: PerformanceQuery) -> AppResult<Performance> {
    db.with(|conn| metrics::performance(conn, &query))
}

/// The home page: summary cards, attempts to resume, due reviews and pending drafts.
#[tauri::command(async)]
pub fn home(db: State<'_, Database>) -> AppResult<HomeSummary> {
    db.with(|conn| metrics::home(conn, &crate::clock::now()))
}

/// The *Erros e revisões* page: counts, and the list of reviews and errors. No answer keys.
#[tauri::command(async)]
pub fn learning_overview(
    db: State<'_, Database>,
    include_blanks: bool,
) -> AppResult<LearningOverview> {
    db.with(|conn| learning::overview(conn, &crate::clock::now(), include_blanks))
}

/// Asks where to save the performance table as CSV (using the same filters) and writes it.
/// Returns `false` if the user cancels.
#[tauri::command]
pub async fn export_performance_csv<R: Runtime>(
    app: AppHandle<R>,
    query: PerformanceQuery,
) -> AppResult<bool> {
    let csv = app
        .state::<Database>()
        .with(|conn| metrics::export_csv(conn, &query))?;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(file) = app
            .dialog()
            .file()
            .set_title("Exportar desempenho")
            .set_file_name("desempenho.csv")
            .add_filter("Planilha CSV", &["csv"])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let path = file
            .into_path()
            .map_err(|_| AppError::Invalid("Escolha um local deste computador.".into()))?;
        std::fs::write(&path, csv)
            .map_err(|_| AppError::Invalid("Não foi possível salvar o arquivo.".into()))?;
        Ok(true)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

/// Starts a session from a collection, a topic, errors or due reviews.
#[tauri::command]
pub fn session_start(db: State<'_, Database>, request: SessionStart) -> AppResult<AttemptView> {
    db.transaction(|tx| attempts::start(tx, &request, &crate::clock::now(), &mut rand::rng()))
}

/// An attempt with every question as the frontend may see it (no unrevealed keys).
#[tauri::command]
pub fn attempt(db: State<'_, Database>, id: AttemptId) -> AppResult<AttemptView> {
    db.with(|conn| attempts::load(conn, &id))
}

/// Chooses an option (`None` clears the choice).
#[tauri::command]
pub fn answer(
    db: State<'_, Database>,
    attempt_id: AttemptId,
    position: u32,
    option_id: Option<String>,
) -> AppResult<AttemptView> {
    db.transaction(|tx| {
        attempts::answer(
            tx,
            &attempt_id,
            position,
            option_id.as_deref(),
            &crate::clock::now(),
        )
    })
}

/// Toggles an option's elimination.
#[tauri::command]
pub fn eliminate(
    db: State<'_, Database>,
    attempt_id: AttemptId,
    position: u32,
    option_id: String,
) -> AppResult<AttemptView> {
    db.transaction(|tx| attempts::eliminate(tx, &attempt_id, position, &option_id))
}

/// Sets or clears a question's flag.
#[tauri::command]
pub fn flag(
    db: State<'_, Database>,
    attempt_id: AttemptId,
    position: u32,
    flagged: bool,
) -> AppResult<AttemptView> {
    db.transaction(|tx| attempts::flag(tx, &attempt_id, position, flagged))
}

/// Finishes an attempt if `revision` is still current.
#[tauri::command]
pub fn finish(
    db: State<'_, Database>,
    attempt_id: AttemptId,
    revision: u32,
) -> AppResult<AttemptView> {
    db.transaction(|tx| attempts::finish(tx, &attempt_id, revision, &crate::clock::now()))
}

/// Annuls a revealed question: it leaves every score until restored.
#[tauri::command]
pub fn annul(
    db: State<'_, Database>,
    attempt_id: AttemptId,
    position: u32,
) -> AppResult<AttemptView> {
    db.transaction(|tx| attempts::annul(tx, &attempt_id, position, &crate::clock::now()))
}

/// Restores an annulled question to every score.
#[tauri::command]
pub fn unannul(
    db: State<'_, Database>,
    attempt_id: AttemptId,
    position: u32,
) -> AppResult<AttemptView> {
    db.transaction(|tx| attempts::restore(tx, &attempt_id, position, &crate::clock::now()))
}

/// Deletes an unfinished attempt.
#[tauri::command]
pub fn abandon(db: State<'_, Database>, attempt_id: AttemptId) -> AppResult<()> {
    db.transaction(|tx| attempts::abandon(tx, &attempt_id))
}

/// Refuses to start AI work with a model the agent's list no longer offers. `roles` picks the
/// configurations the work will use from the saved settings. Uses only lists that are already
/// loaded, so it never starts a CLI.
fn ensure_roles_listed(
    db: &Database,
    cache: &CatalogCache,
    roles: impl FnOnce(&RoleSettings) -> Vec<(crate::ai::roles::RoleConfig, &'static str)>,
) -> AppResult<()> {
    if cache.cached().is_none() {
        return Ok(());
    }
    let saved = db.with(|conn| settings::load_roles(conn))?;
    let roles = roles(&saved);
    let refs: Vec<_> = roles
        .iter()
        .map(|(config, label)| (config, *label))
        .collect();
    model_catalog::ensure_cached_listed(cache, &refs)
}

/// The AI settings (CLI paths, roles, RemNote). Writes the defaults on first use.
#[tauri::command]
pub fn ai_settings(db: State<'_, Database>, secrets: State<'_, Secrets>) -> AppResult<AiSettings> {
    db.with(|conn| settings::load(conn, &secrets))
}

/// The models and efforts each agent offers, from the agents' own CLIs. The lists are cached for
/// the session; `refresh` reads them again.
#[tauri::command(async)]
pub fn ai_catalog(
    db: State<'_, Database>,
    cache: State<'_, CatalogCache>,
    refresh: bool,
) -> AppResult<model_catalog::Catalog> {
    let cli_paths = db.with(|conn| settings::load_cli_paths(conn))?;
    Ok(model_catalog::catalog(&cache, &cli_paths, refresh))
}

/// Validates and saves the AI settings, and returns them as saved.
#[tauri::command]
pub fn save_ai_settings(
    db: State<'_, Database>,
    secrets: State<'_, Secrets>,
    catalog: State<'_, CatalogCache>,
    input: AiSettingsInput,
) -> AppResult<AiSettings> {
    let saved = db.with(|conn| {
        settings::save(conn, input)?;
        settings::load(conn, &secrets)
    })?;
    // The CLI paths may have changed, so the model lists are read again on next use.
    catalog.clear();
    Ok(saved)
}

/// Saves the RemNote token in the OS keychain; `None` or blank removes it. Returns whether a
/// token is saved. The token is never returned.
#[tauri::command]
pub fn set_remnote_token(secrets: State<'_, Secrets>, token: Option<String>) -> AppResult<bool> {
    secrets.set_remnote_token(token.as_deref())?;
    Ok(secrets.remnote_token()?.is_some())
}

/// Finds the CLI (using the saved path override) and reports its version and login state.
#[tauri::command(async)]
pub fn cli_test(
    db: State<'_, Database>,
    secrets: State<'_, Secrets>,
    agent: Agent,
) -> AppResult<CliStatus> {
    let settings = db.with(|conn| settings::load(conn, &secrets))?;
    let custom = settings.cli_paths.get(agent).map(std::path::Path::new);
    Ok(discovery::cli_test(agent, custom))
}

/// Checks the saved RemNote connection and lists the tools the server offers.
#[tauri::command(async)]
pub fn remnote_test(
    db: State<'_, Database>,
    secrets: State<'_, Secrets>,
) -> AppResult<RemNoteTest> {
    let settings = db.with(|conn| settings::load(conn, &secrets))?;
    settings::remnote_test(&settings, &secrets)
}

/// Name of the window event that carries every [`GenerationStatus`] change.
pub const GENERATION_EVENT: &str = "generation-status";

/// Starts a generation in the background. Fails at once if the form is invalid or another
/// generation is running. Progress and the result arrive as [`GENERATION_EVENT`] events.
#[tauri::command]
pub fn generation_start<R: Runtime>(
    app: AppHandle<R>,
    manager: State<'_, GenerationManager>,
    service: State<'_, BackupService>,
    paths: State<'_, AppPaths>,
    mut request: GenerateRequest,
) -> AppResult<()> {
    ensure_not_backing_up(&service)?;
    request.validate()?;
    ensure_roles_listed(
        &app.state::<Database>(),
        &app.state::<CatalogCache>(),
        |roles| {
            vec![
                (
                    request
                        .generator
                        .clone()
                        .unwrap_or_else(|| roles.generate.clone()),
                    "Gerar questões",
                ),
                (
                    request
                        .checker
                        .clone()
                        .unwrap_or_else(|| roles.blind_check.clone()),
                    "Conferência às cegas",
                ),
            ]
        },
    )?;
    let files_dir = paths.draft_files_dir();
    let worker = app.clone();
    manager.start(
        GenerationKind::Generate,
        move |cancel, progress| {
            let db = worker.state::<Database>();
            let secrets = worker.state::<Secrets>();
            blind_check::generate_and_check(&db, &secrets, &files_dir, request, cancel, progress)
        },
        move |status| {
            let _ = app.emit(GENERATION_EVENT, status);
        },
    )
}

/// Runs the blind check again on a draft whose check failed. Uses the same background slot as
/// a generation, so only one runs at a time.
#[tauri::command]
pub fn blind_check_start<R: Runtime>(
    app: AppHandle<R>,
    manager: State<'_, GenerationManager>,
    service: State<'_, BackupService>,
    paths: State<'_, AppPaths>,
    draft_id: DraftId,
) -> AppResult<()> {
    ensure_not_backing_up(&service)?;
    ensure_roles_listed(
        &app.state::<Database>(),
        &app.state::<CatalogCache>(),
        |roles| vec![(roles.blind_check.clone(), "Conferência às cegas")],
    )?;
    let files_dir = paths.draft_files_dir();
    let worker = app.clone();
    manager.start(
        GenerationKind::Recheck,
        move |cancel, progress| {
            let db = worker.state::<Database>();
            let secrets = worker.state::<Secrets>();
            blind_check::recheck_draft(&db, &secrets, &files_dir, &draft_id, cancel, progress)
        },
        move |status| {
            let _ = app.emit(GENERATION_EVENT, status);
        },
    )
}

/// Drafts waiting for review, newest first. Counts and topics only.
#[tauri::command]
pub fn drafts_waiting(db: State<'_, Database>) -> AppResult<Vec<DraftListItem>> {
    db.with(|conn| review::waiting(conn))
}

/// The spoiler-free summary of a draft: counts, problems and one state per question (plus the stem and options of flagged ones).
#[tauri::command]
pub fn draft_summary(db: State<'_, Database>, id: DraftId) -> AppResult<DraftSummary> {
    db.with(|conn| review::summary(conn, &id))
}

/// Imports a draft as a new `generated` collection: every question, or only the confirmed ones.
#[tauri::command]
pub fn draft_import(
    db: State<'_, Database>,
    paths: State<'_, AppPaths>,
    id: DraftId,
    mode: ImportMode,
) -> AppResult<ImportResult> {
    let result = db.transaction(|tx| review::import(tx, &id, mode, &crate::clock::now()))?;
    draft_files::remove(&paths.draft_files_dir(), &id);
    Ok(result)
}

/// Discards a draft that is waiting for review.
#[tauri::command]
pub fn draft_discard(
    db: State<'_, Database>,
    paths: State<'_, AppPaths>,
    id: DraftId,
) -> AppResult<()> {
    db.with(|conn| review::discard(conn, &id))?;
    draft_files::remove(&paths.draft_files_dir(), &id);
    Ok(())
}

/// Asks which PDF, Markdown or text files to attach to a generation. Empty if cancelled.
#[tauri::command]
pub async fn pick_attachments<R: Runtime>(app: AppHandle<R>) -> AppResult<Vec<String>> {
    tauri::async_runtime::spawn_blocking(move || {
        let files = app
            .dialog()
            .file()
            .set_title("Anexar arquivos")
            .add_filter("PDF, Markdown ou texto", &["pdf", "md", "txt"])
            .blocking_pick_files()
            .unwrap_or_default();
        files
            .into_iter()
            .map(|file| {
                file.into_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .map_err(|_| AppError::Invalid("Escolha arquivos deste computador.".into()))
            })
            .collect()
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

/// Asks the re-check role whether the key of a revealed question is right. Waits for the AI;
/// the proposal stays in the backend until it is applied or dismissed.
#[tauri::command]
pub async fn recheck_ask<R: Runtime>(
    app: AppHandle<R>,
    attempt_id: AttemptId,
    position: u32,
) -> AppResult<RecheckView> {
    tauri::async_runtime::spawn_blocking(move || {
        let db = app.state::<Database>();
        let secrets = app.state::<Secrets>();
        let jobs = app.state::<OnDemand>();
        let catalog = app.state::<CatalogCache>();
        ensure_roles_listed(&db, &catalog, |roles| {
            vec![(roles.recheck.clone(), "Revisão sob demanda")]
        })?;
        recheck::ask_recheck(&db, &secrets, &jobs, &attempt_id, position)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

/// Applies the key the AI proposed, after the user confirmed. Past attempts are re-scored.
#[tauri::command]
pub fn recheck_apply(
    db: State<'_, Database>,
    jobs: State<'_, OnDemand>,
    attempt_id: AttemptId,
    position: u32,
) -> AppResult<AttemptView> {
    db.transaction(|tx| recheck::apply(tx, &jobs, &attempt_id, position, &crate::clock::now()))
}

/// Drops the AI's proposal and keeps the current key.
#[tauri::command]
pub fn recheck_dismiss(
    db: State<'_, Database>,
    jobs: State<'_, OnDemand>,
    attempt_id: AttemptId,
    position: u32,
) -> AppResult<()> {
    db.with(|conn| recheck::dismiss(conn, &jobs, &attempt_id, position))
}

/// Writes the missing explanation of a revealed question with the AI and returns the attempt.
/// Waits for the AI.
#[tauri::command]
pub async fn explain_ask<R: Runtime>(
    app: AppHandle<R>,
    attempt_id: AttemptId,
    position: u32,
) -> AppResult<AttemptView> {
    tauri::async_runtime::spawn_blocking(move || {
        let db = app.state::<Database>();
        let secrets = app.state::<Secrets>();
        let jobs = app.state::<OnDemand>();
        let catalog = app.state::<CatalogCache>();
        ensure_roles_listed(&db, &catalog, |roles| {
            vec![(roles.explain.clone(), "Explicação sob demanda")]
        })?;
        explain::ask_explain(&db, &secrets, &jobs, &attempt_id, position)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

/// Cancels the on-demand AI request that is running (re-check or explain).
#[tauri::command]
pub fn ai_request_cancel(jobs: State<'_, OnDemand>) -> bool {
    jobs.cancel()
}

/// Stops the running generation. Returns whether one was running.
#[tauri::command]
pub fn generation_cancel(manager: State<'_, GenerationManager>) -> bool {
    manager.cancel()
}

/// What the generation is doing now, or how the last one ended.
#[tauri::command]
pub fn generation_status(manager: State<'_, GenerationManager>) -> GenerationStatus {
    manager.status()
}

/// Forgets how the last generation ended.
#[tauri::command]
pub fn generation_dismiss(manager: State<'_, GenerationManager>) {
    manager.dismiss();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder, mock_context, noop_assets};
    use tauri::webview::InvokeRequest;

    /// Sends `cmd` through Tauri's IPC layer on a mock runtime and returns the JSON result.
    fn invoke(cmd: &str) -> Result<Value, Value> {
        invoke_with(cmd, json!({}), Database::open_in_memory().unwrap())
    }

    /// Like [`invoke`], with JSON arguments and the given database as managed state.
    fn invoke_with(cmd: &str, args: Value, db: Database) -> Result<Value, Value> {
        MockApp::new(db).call(cmd, args)
    }

    /// A mock app with a database, for tests that send several commands in a row.
    struct MockApp {
        _data: tempfile::TempDir,
        _app: tauri::App<tauri::test::MockRuntime>,
        webview: tauri::WebviewWindow<tauri::test::MockRuntime>,
    }

    impl MockApp {
        fn new(db: Database) -> Self {
            Self::with_catalog(
                db,
                crate::ai::catalog::CatalogCache::new(vec![Box::new(
                    crate::ai::catalog::claude::ClaudeSource,
                )]),
            )
        }

        /// Like [`MockApp::new`], with the given model lists as managed state.
        fn with_catalog(db: Database, catalog: crate::ai::catalog::CatalogCache) -> Self {
            let app = crate::register(mock_builder())
                .build(mock_context(noop_assets()))
                .expect("mock app builds");
            app.manage(db);
            app.manage(Secrets::memory());
            app.manage(catalog);
            app.manage(BackupService::default());
            let data = tempfile::tempdir().unwrap();
            app.manage(AppPaths::new(data.path()));
            app.manage(GenerationManager::default());
            app.manage(OnDemand::default());
            let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
                .build()
                .expect("mock webview builds");
            Self {
                _data: data,
                _app: app,
                webview,
            }
        }

        /// Sends `cmd` through Tauri's IPC layer and returns the JSON result.
        fn call(&self, cmd: &str, args: Value) -> Result<Value, Value> {
            let url = if cfg!(windows) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            };
            get_ipc_response(
                &self.webview,
                InvokeRequest {
                    cmd: cmd.into(),
                    callback: CallbackFn(0),
                    error: CallbackFn(1),
                    url: url.parse().unwrap(),
                    body: InvokeBody::Json(args),
                    headers: Default::default(),
                    invoke_key: INVOKE_KEY.to_string(),
                },
            )
            .map(|body| body.deserialize::<Value>().unwrap())
        }
    }

    #[test]
    fn a_database_failure_returns_the_generic_ipc_error() {
        let db = Database::open_in_memory().unwrap();
        db.with(|conn| {
            conn.execute("DROP TABLE collections", [])?;
            Ok(())
        })
        .unwrap();

        let result = MockApp::new(db).call("collections", json!({ "query": {} }));

        assert_eq!(
            result,
            Err(json!({ "message": "Ocorreu um erro inesperado. Tente novamente." }))
        );
    }

    #[test]
    fn browsing_commands_decode_camel_case_arguments() {
        let db = Database::open_in_memory().unwrap();
        db.with(|c| {
            c.execute(
                "INSERT INTO collections (id, title, kind, contest, created_at) VALUES
                   ('c1', 'Direito › Crase', ?1, 'ALEPA', '2026-11-19T00:00:00.000Z'),
                   ('c2', 'Informática › SQL', ?1, 'Perito', '2026-11-19T00:00:00.000Z')",
                [crate::bank::catalog::CollectionKind::Imported],
            )?;
            Ok(())
        })
        .unwrap();

        let page = invoke_with(
            "collections",
            json!({ "query": { "search": "CRASE", "pageSize": 5 } }),
            db,
        )
        .expect("collections succeeds");
        assert_eq!(page["total"], json!(1));
        assert_eq!(page["pageSize"], json!(5));
        assert_eq!(page["items"][0]["id"], json!("c1"));
        assert_eq!(page["contests"], json!(["ALEPA", "Perito"]));

        let missing = invoke_with(
            "collection",
            json!({ "id": "nope" }),
            Database::open_in_memory().unwrap(),
        );
        assert_eq!(
            missing,
            Err(json!({ "message": "Caderno não encontrado." }))
        );

        let tree = invoke_with(
            "topics_tree",
            json!({}),
            Database::open_in_memory().unwrap(),
        );
        assert_eq!(tree, Ok(json!({ "areas": [], "questions": 0 })));
    }

    /// Two questions in collection `c1`, each with a distinctive explanation and reference.
    fn solving_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.transaction(|tx| {
            let mut inputs = vec![
                crate::bank::question::tests::boolean("q1", "Primeira", true),
                crate::bank::question::tests::boolean("q2", "Segunda", false),
            ];
            for input in &mut inputs {
                input.explanation = format!("SEGREDO-{}", input.local_id);
                input.refs = vec![format!("REF-{}", input.local_id)];
            }
            let ids = crate::bank::import_questions(tx, inputs, "2026-11-20T00:00:00.000Z")?
                .ids_by_local_id;
            tx.execute(
                "INSERT INTO collections (id, title, kind, created_at)
                 VALUES ('c1', 'Caderno', ?1, '2026-11-20T00:00:00.000Z')",
                [crate::bank::catalog::CollectionKind::Imported],
            )?;
            tx.execute(
                "INSERT INTO collection_items VALUES ('c1', 0, ?1), ('c1', 1, ?2)",
                rusqlite::params![ids["q1"], ids["q2"]],
            )?;
            Ok(())
        })
        .unwrap();
        db
    }

    fn start(app: &MockApp, mode: &str) -> Value {
        app.call(
            "session_start",
            json!({ "request": {
                "source": { "kind": "collection", "collectionId": "c1" },
                "feedbackMode": mode
            } }),
        )
        .expect("session starts")
    }

    #[test]
    fn topic_attempt_ipc_preserves_scalar_ids() {
        let db = solving_db();
        let topic_id: crate::ids::TopicId = db
            .with(|conn| Ok(conn.query_row("SELECT id FROM topics", [], |r| r.get(0))?))
            .unwrap();
        let app = MockApp::new(db);
        let view = app
            .call(
                "session_start",
                json!({ "request": {
                    "source": { "kind": "topic", "topicId": topic_id, "count": 1 },
                    "feedbackMode": "study"
                } }),
            )
            .expect("topic session starts");
        assert_eq!(view["sourceRef"], json!(topic_id.to_string()));
        assert!(view["id"].is_string());
        assert!(view["questions"][0]["questionId"].is_string());
    }

    /// The question's answer, explanation and references are absent from the JSON.
    fn assert_hidden(question: &Value) {
        assert_eq!(question["reveal"], Value::Null, "{question}");
        let text = question.to_string();
        for secret in ["SEGREDO", "REF-", "answerId", "outcome"] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
    }

    #[test]
    fn learning_overview_decodes_camel_case_and_has_no_question_content() {
        let app = MockApp::new(solving_db());
        let overview = app
            .call("learning_overview", json!({ "includeBlanks": true }))
            .expect("overview succeeds");
        assert_eq!(overview["due"], json!(0));
        assert_eq!(overview["errors"], json!(0));
        assert_eq!(overview["items"], json!([]));
        let missing = app.call("learning_overview", json!({}));
        assert!(missing.is_err(), "includeBlanks is required");
    }

    #[test]
    fn home_shows_active_attempts_without_any_question_content() {
        let app = MockApp::new(solving_db());
        start(&app, "exam");
        let home = app.call("home", json!({})).expect("home succeeds");
        let attempts = home["activeAttempts"].as_array().expect("a list");
        assert_eq!(attempts.len(), 1, "{home}");
        assert_eq!(attempts[0]["title"], json!("Caderno"));
        assert_eq!(attempts[0]["total"], json!(2));
        assert_eq!(attempts[0]["answered"], json!(0));
        let text = home.to_string();
        for secret in [
            "SEGREDO",
            "REF-",
            "answerId",
            "explanation",
            "stem",
            "options",
            "Primeira",
            "Segunda",
            "reveal",
        ] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
    }

    #[test]
    fn exam_commands_reveal_nothing_until_finish() {
        let app = MockApp::new(solving_db());
        let view = start(&app, "exam");
        assert_eq!(view["title"], json!("Caderno"));
        assert_eq!(view["questions"][0]["type"], json!("boolean"));
        assert_eq!(view["score"], Value::Null);
        let id = view["id"].clone();
        let option = view["questions"][0]["options"][0]["id"].clone();

        let answered = app
            .call(
                "answer",
                json!({ "attemptId": id, "position": 0, "optionId": option }),
            )
            .expect("answer succeeds");
        assert_eq!(answered["questions"][0]["choice"], option);
        let reloaded = app
            .call("attempt", json!({ "id": id }))
            .expect("attempt loads");
        for view in [&answered, &reloaded] {
            assert_eq!(view["score"], Value::Null);
            view["questions"]
                .as_array()
                .unwrap()
                .iter()
                .for_each(assert_hidden);
        }

        let finished = app
            .call(
                "finish",
                json!({ "attemptId": id, "revision": reloaded["revision"] }),
            )
            .expect("finish succeeds");
        assert!(
            finished["questions"][0]["reveal"]["explanation"]
                .as_str()
                .unwrap()
                .starts_with("SEGREDO")
        );
        assert_eq!(finished["score"]["evaluated"], json!(2));
    }

    #[test]
    fn study_commands_reveal_only_the_answered_question() {
        let app = MockApp::new(solving_db());
        let view = start(&app, "study");
        let id = view["id"].clone();
        let option = view["questions"][0]["options"][0]["id"].clone();

        app.call(
            "answer",
            json!({ "attemptId": id, "position": 0, "optionId": option }),
        )
        .expect("answer succeeds");
        let reloaded = app
            .call("attempt", json!({ "id": id }))
            .expect("attempt loads");
        let reveal = &reloaded["questions"][0]["reveal"];
        assert!(reveal["answerId"].is_string());
        assert!(
            reveal["explanation"]
                .as_str()
                .unwrap()
                .starts_with("SEGREDO")
        );
        assert_hidden(&reloaded["questions"][1]);
        assert_eq!(reloaded["score"], Value::Null);

        // The revealed choice is locked, also through IPC.
        let other = view["questions"][0]["options"][1]["id"].clone();
        let locked = app.call(
            "answer",
            json!({ "attemptId": id, "position": 0, "optionId": other }),
        );
        assert_eq!(
            locked,
            Err(json!({ "message": "A resposta desta questão já foi revelada e não pode mudar." }))
        );
    }

    #[test]
    fn ai_commands_need_a_revealed_question() {
        let app = MockApp::new(solving_db());
        let view = start(&app, "study");
        let id = view["id"].clone();
        for command in [
            "recheck_ask",
            "recheck_apply",
            "recheck_dismiss",
            "explain_ask",
        ] {
            let early = app.call(command, json!({ "attemptId": id, "position": 0 }));
            assert_eq!(
                early,
                Err(json!({ "message": "Só é possível usar a IA depois de ver o gabarito." })),
                "{command}"
            );
        }
    }

    #[test]
    fn annul_commands_need_a_revealed_question() {
        let app = MockApp::new(solving_db());
        let view = start(&app, "study");
        let id = view["id"].clone();
        let option = view["questions"][0]["options"][0]["id"].clone();

        let early = app.call("annul", json!({ "attemptId": id, "position": 0 }));
        assert_eq!(
            early,
            Err(json!({ "message": "Só é possível anular uma questão depois de ver o gabarito." }))
        );

        app.call(
            "answer",
            json!({ "attemptId": id, "position": 0, "optionId": option }),
        )
        .expect("answer succeeds");
        let annulled = app
            .call("annul", json!({ "attemptId": id, "position": 0 }))
            .expect("annul succeeds");
        assert_eq!(annulled["questions"][0]["reveal"]["annulled"], json!(true));
        assert_eq!(
            annulled["questions"][0]["reveal"]["outcome"],
            json!("annulled")
        );
        assert_hidden(&annulled["questions"][1]);

        let restored = app
            .call("unannul", json!({ "attemptId": id, "position": 0 }))
            .expect("unannul succeeds");
        assert_eq!(restored["questions"][0]["reveal"]["annulled"], json!(false));
        assert_hidden(&restored["questions"][1]);
    }

    #[test]
    fn app_info_reports_name_and_crate_version() {
        let info = current_app_info();
        assert_eq!(info.name, "Gabaritei");
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn performance_decodes_filters_and_returns_camel_case() {
        let db = Database::open_in_memory().unwrap();
        let performance = invoke_with(
            "performance",
            json!({ "query": { "contest": "TRT", "feedbackMode": "study", "utcOffsetMinutes": -180 } }),
            db,
        )
        .expect("performance succeeds");
        assert_eq!(performance["overall"]["score"]["evaluated"], json!(0));
        assert_eq!(performance["overall"]["smallSample"], json!(true));
        assert_eq!(performance["trend"], json!([]));
        assert_eq!(performance["filters"]["contests"], json!([]));
    }

    #[test]
    fn backup_commands_make_list_and_restore_a_backup() {
        let app = MockApp::new(Database::open_in_memory().unwrap());
        let status = app.call("backup_status", json!({})).expect("status");
        assert_eq!(status["isDefaultFolder"], json!(true));
        assert_eq!(status["backups"], json!([]));

        // The in-memory database has no file to back up from, but the online backup copies it.
        let made = app.call("backup_now", json!({})).expect("backs up");
        let name = made["name"].as_str().unwrap().to_owned();
        assert_eq!(made["kind"], json!("manual"));
        let status = app.call("backup_status", json!({})).expect("status");
        assert_eq!(status["backups"][0]["name"], json!(name));
        assert_eq!(status["lastError"], json!(null));

        app.call("backup_restore", json!({ "name": name }))
            .expect("restores");
        let status = app.call("backup_status", json!({})).expect("status");
        assert_eq!(status["backups"].as_array().unwrap().len(), 2);
        assert_eq!(status["backups"][0]["kind"], json!("safety"));

        let error = app
            .call("backup_restore", json!({ "name": "../escape" }))
            .unwrap_err();
        assert_eq!(error["message"], json!("Backup não encontrado."));
    }

    #[test]
    fn ai_catalog_returns_each_agents_list_in_camel_case() {
        let app = MockApp::new(Database::open_in_memory().unwrap());
        let catalog = app
            .call("ai_catalog", json!({ "refresh": false }))
            .expect("loads");
        assert_eq!(catalog["agents"][0]["agent"], json!("claude"));
        assert_eq!(catalog["agents"][0]["status"]["kind"], json!("unavailable"));
        assert!(
            catalog["agents"][0]["status"]["reason"]
                .as_str()
                .unwrap()
                .contains("Claude Code")
        );
        assert_eq!(catalog["agents"][0]["models"], json!([]));
    }

    #[test]
    fn ai_settings_default_save_and_never_return_the_token() {
        let app = MockApp::new(Database::open_in_memory().unwrap());
        let settings = app.call("ai_settings", json!({})).expect("loads");
        assert_eq!(settings["roles"]["generate"]["model"], json!("gpt-6.2-sol"));
        assert_eq!(settings["roles"]["generate"]["agent"], json!("codex"));
        assert_eq!(settings["roles"]["blindCheck"]["effort"], json!("xhigh"));
        assert_eq!(settings["hasRemnoteToken"], json!(false));

        let saved = app
            .call("set_remnote_token", json!({ "token": "TOKEN-SECRETO" }))
            .expect("token saves");
        assert_eq!(saved, json!(true));

        let mut input = json!({
            "cliPaths": { "claude": null, "codex": null },
            "roles": settings["roles"],
            "remnote": { "mode": "server", "url": "https://r.example/mcp", "tools": ["search"] },
        });
        input["roles"]["explain"]["agent"] = json!("claude");
        input["roles"]["explain"]["model"] = json!("claude-sonnet-5-5");
        input["roles"]["explain"]["effort"] = json!("medium");
        let after = app
            .call("save_ai_settings", json!({ "input": input }))
            .expect("saves");
        assert_eq!(after["roles"]["explain"]["agent"], json!("claude"));
        assert_eq!(after["remnote"]["tools"], json!(["search"]));
        assert_eq!(after["hasRemnoteToken"], json!(true));
        assert!(!after.to_string().contains("TOKEN-SECRETO"));

        let reloaded = app.call("ai_settings", json!({})).expect("reloads");
        assert_eq!(
            reloaded["roles"]["explain"]["model"],
            json!("claude-sonnet-5-5")
        );
        assert!(!reloaded.to_string().contains("TOKEN-SECRETO"));

        let removed = app
            .call("set_remnote_token", json!({ "token": "" }))
            .expect("token clears");
        assert_eq!(removed, json!(false));
    }

    #[test]
    fn saving_invalid_ai_settings_returns_a_pt_br_message() {
        let app = MockApp::new(Database::open_in_memory().unwrap());
        let mut settings = app.call("ai_settings", json!({})).unwrap();
        settings["roles"]["generate"]["model"] = json!("");
        let input = json!({
            "cliPaths": settings["cliPaths"], "roles": settings["roles"], "remnote": settings["remnote"],
        });
        let error = app
            .call("save_ai_settings", json!({ "input": input }))
            .expect_err("a blank model is refused");
        assert!(
            error["message"]
                .as_str()
                .unwrap()
                .contains("Informe o modelo")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_generation_runs_in_the_background_and_ends_with_a_checked_draft() {
        let dir = tempfile::tempdir().unwrap();
        // Call 1 is the generator, call 2 the blind checker (which agrees with the key).
        let package = json!({"questions": [{
            "id": "q1", "type": "boolean", "stem": "A crase é obrigatória antes de verbos.",
            "options": [{"id": "c", "text": "Certo"}, {"id": "e", "text": "Errado"}],
            "answerId": "e", "explanation": "Não se usa crase antes de verbo.", "refs": []
        }], "notes": []});
        let verdicts = json!({"verdicts": [
            {"questionId": "q1", "choice": "e", "confidence": "high", "reason": "Verbo não aceita crase."}
        ]});
        std::fs::write(dir.path().join("answer-1.json"), package.to_string()).unwrap();
        std::fs::write(dir.path().join("answer-2.json"), verdicts.to_string()).unwrap();
        let cli = crate::ai::runner::tests::write_script(
            dir.path(),
            "codex",
            &format!(
                "dir='{}'\nn=$(cat \"$dir/counter\" 2>/dev/null || echo 0); n=$((n+1)); echo $n > \"$dir/counter\"\nout=\"\"\nwhile [ $# -gt 0 ]; do case \"$1\" in -o) out=\"$2\"; shift;; esac; shift; done\ncat >/dev/null\ncp \"$dir/answer-$n.json\" \"$out\"",
                dir.path().display()
            ),
        );
        let app = MockApp::new(Database::open_in_memory().unwrap());
        let mut settings = app.call("ai_settings", json!({})).unwrap();
        settings["cliPaths"]["codex"] = json!(cli.to_string_lossy());
        let input = json!({
            "cliPaths": settings["cliPaths"], "roles": settings["roles"], "remnote": settings["remnote"],
        });
        app.call("save_ai_settings", json!({ "input": input }))
            .unwrap();

        let request = json!({
            "topic": { "area": "Português", "matter": "Gramática", "topic": "Crase" },
            "count": 1, "board": null, "format": "boolean", "instructions": null,
            "sources": { "remnote": false, "web": false, "attachments": [] },
            "generator": null, "checker": null,
        });
        app.call("generation_start", json!({ "request": request }))
            .expect("starts");

        let mut status = json!(null);
        for _ in 0..200 {
            status = app.call("generation_status", json!({})).unwrap();
            if status["running"] == json!(false) && !status["outcome"].is_null() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert_eq!(status["outcome"]["kind"], json!("draft"), "{status}");
        assert!(status["outcome"]["draftId"].as_str().unwrap().len() > 8);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("counter"))
                .unwrap()
                .trim(),
            "2"
        );
        // The status carries labels and an ID, never question content.
        assert!(!status.to_string().contains("crase"));

        // The preview shows counts and states, plus the stem and options of flagged questions only: never the key, explanation or references.
        let id = status["outcome"]["draftId"].as_str().unwrap().to_owned();
        let summary = app
            .call("draft_summary", json!({ "id": id }))
            .expect("summary");
        assert_eq!(summary["count"], json!(1));
        assert_eq!(
            summary["lines"],
            json!([{ "position": 1, "state": "agreed", "hasProblem": false, "content": null }])
        );
        let shown = summary.to_string();
        for secret in [
            "crase",
            "verbo",
            "Certo",
            "Errado",
            "answerId",
            "explanation",
        ] {
            assert!(!shown.contains(secret), "{secret} leaked: {shown}");
        }
        let waiting = app.call("drafts_waiting", json!({})).unwrap();
        assert_eq!(waiting[0]["id"], json!(id));
        assert!(!waiting.to_string().contains("crase"));

        // Importing makes a generated collection and closes the draft.
        let imported = app
            .call("draft_import", json!({ "id": id, "mode": "all" }))
            .expect("imports");
        assert_eq!(imported["imported"], json!(1));
        let collections = app
            .call(
                "collections",
                json!({ "query": { "search": "", "contest": "", "board": "", "page": 0, "pageSize": 20 } }),
            )
            .expect("collections load");
        assert_eq!(collections["items"][0]["kind"], json!("generated"));
        assert_eq!(collections["items"][0]["questions"], json!(1));
        assert!(
            app.call("draft_import", json!({ "id": id, "mode": "all" }))
                .is_err()
        );
        assert_eq!(app.call("drafts_waiting", json!({})).unwrap(), json!([]));

        app.call("generation_dismiss", json!({})).unwrap();
        assert_eq!(
            app.call("generation_status", json!({})).unwrap()["outcome"],
            json!(null)
        );
    }

    /// A source that always lists one Codex model, so a run can be checked against it.
    #[derive(Default)]
    struct OneModel(std::sync::Arc<std::sync::atomic::AtomicUsize>);

    impl crate::ai::catalog::CatalogSource for OneModel {
        fn agent(&self) -> Agent {
            Agent::Codex
        }

        fn load(&self, _: &settings::CliPaths) -> crate::ai::catalog::AgentCatalog {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            crate::ai::catalog::AgentCatalog {
                agent: Agent::Codex,
                status: crate::ai::catalog::CatalogStatus::Ready,
                models: vec![crate::ai::catalog::ModelOption {
                    id: "m1".into(),
                    label: "M1".into(),
                    efforts: vec![crate::ai::Effort::Low],
                    supports_fast: false,
                }],
            }
        }
    }

    #[test]
    fn a_generation_with_a_model_the_catalog_no_longer_lists_is_refused() {
        let cache = crate::ai::catalog::CatalogCache::new(vec![Box::new(OneModel::default())]);
        cache.get(&settings::CliPaths::default());
        let app = MockApp::with_catalog(Database::open_in_memory().unwrap(), cache);
        let request = json!({
            "topic": { "area": "Português", "matter": "Gramática", "topic": "Crase" },
            "count": 1, "board": null, "format": null, "instructions": null,
            "sources": { "remnote": false, "web": false, "attachments": [] },
            "generator": { "agent": "codex", "model": "gone", "effort": "low" }, "checker": null,
        });
        let error = app
            .call("generation_start", json!({ "request": request }))
            .expect_err("the stale model is refused");
        let message = error["message"].as_str().unwrap();
        assert!(message.contains("não está mais disponível"), "{message}");
        assert!(message.contains("Gerar questões"), "{message}");
        let status = app.call("generation_status", json!({})).unwrap();
        assert_eq!(status["running"], json!(false));
    }

    #[test]
    fn saving_the_settings_makes_the_next_catalog_read_load_again() {
        use std::sync::atomic::Ordering;
        let loads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let cache = crate::ai::catalog::CatalogCache::new(vec![Box::new(OneModel(loads.clone()))]);
        let app = MockApp::with_catalog(Database::open_in_memory().unwrap(), cache);
        let before = 0;
        app.call("ai_catalog", json!({ "refresh": false })).unwrap();
        app.call("ai_catalog", json!({ "refresh": false })).unwrap();
        assert_eq!(loads.load(Ordering::SeqCst), before + 1);

        let settings = app.call("ai_settings", json!({})).unwrap();
        let input = json!({
            "cliPaths": settings["cliPaths"], "roles": settings["roles"], "remnote": settings["remnote"],
        });
        app.call("save_ai_settings", json!({ "input": input }))
            .unwrap();

        app.call("ai_catalog", json!({ "refresh": false })).unwrap();
        assert_eq!(loads.load(Ordering::SeqCst), before + 2);
    }

    #[test]
    fn an_invalid_generation_form_is_refused_before_anything_starts() {
        let app = MockApp::new(Database::open_in_memory().unwrap());
        let request = json!({
            "topic": { "area": "Direito", "matter": "", "topic": "Atos" },
            "count": 5, "board": null, "format": null, "instructions": null,
            "sources": { "remnote": false, "web": false, "attachments": [] },
            "generator": null, "checker": null,
        });
        let error = app
            .call("generation_start", json!({ "request": request }))
            .expect_err("a blank matter is refused");
        assert!(error["message"].as_str().unwrap().contains("matéria"));
        let status = app.call("generation_status", json!({})).unwrap();
        assert_eq!(status["running"], json!(false));
        assert_eq!(status["outcome"], json!(null));
        assert_eq!(
            app.call("generation_cancel", json!({})).unwrap(),
            json!(false)
        );
    }

    #[test]
    fn app_info_round_trips_through_ipc_as_camel_case_json() {
        let response = invoke("app_info").expect("command succeeds");
        assert_eq!(
            response,
            json!({ "name": "Gabaritei", "version": env!("CARGO_PKG_VERSION") })
        );
    }

    #[test]
    fn appearance_defaults_then_saves_through_ipc() {
        let app = MockApp::new(Database::open_in_memory().unwrap());
        assert_eq!(
            app.call("appearance", json!({})).expect("loads"),
            json!({ "theme": "system", "textSize": "m" })
        );
        let saved = app
            .call(
                "save_appearance",
                json!({ "appearance": { "theme": "dark", "textSize": "l" } }),
            )
            .expect("saves");
        assert_eq!(saved, json!({ "theme": "dark", "textSize": "l" }));
        assert_eq!(app.call("appearance", json!({})).expect("reloads"), saved);
        app.call(
            "save_appearance",
            json!({ "appearance": { "theme": "neon", "textSize": "l" } }),
        )
        .expect_err("an unknown theme is refused");
    }

    #[test]
    fn keyboard_shortcuts_default_on_then_save_through_ipc() {
        let app = MockApp::new(Database::open_in_memory().unwrap());
        assert_eq!(
            app.call("keyboard_shortcuts", json!({})).expect("loads"),
            json!(true)
        );
        let saved = app
            .call("save_keyboard_shortcuts", json!({ "enabled": false }))
            .expect("saves");
        assert_eq!(saved, json!(false));
        assert_eq!(
            app.call("keyboard_shortcuts", json!({})).expect("reloads"),
            json!(false)
        );
    }

    #[test]
    fn unknown_command_is_rejected() {
        let error = invoke("does_not_exist").expect_err("unknown command fails");
        assert_eq!(error, json!("Command does_not_exist not found"));
    }
}
