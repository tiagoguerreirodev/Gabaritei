//! Tauri commands: thin wrappers that call domain functions.
//!
//! Every command must also be added to [`crate::register`] and wrapped in `src/api.ts`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_dialog::{DialogExt, FilePath};
use ts_rs::TS;

use crate::ai::agent::Agent;
use crate::ai::blind_check;
use crate::ai::catalog::{self as model_catalog, CatalogCache};
use crate::ai::discovery::{self, CliStatus};
use crate::ai::explain;
use crate::ai::generate::{GenerateRequest, GenerationRuntime};
use crate::ai::jobs::{self, DataReplacement, GenerationKind, GenerationManager, GenerationStatus};
use crate::ai::ondemand::OnDemand;
use crate::ai::recheck::{self, RecheckView};
use crate::ai::remnote_probe::{self, RemNoteTest};
use crate::ai::review::{
    self, DraftListItem, DraftSummary, ImportAllOutcome, ImportMode, ImportResult,
};
use crate::ai::roles::{self, Role, RoleConfig};
use crate::ai::secrets::Secrets;
use crate::ai::settings::{self, AiSettings, AiSettingsInput, ResolvedClis};
use crate::appearance::{self, Appearance};
use crate::attempts::{self, ActiveAttempt, AttemptView, HistoryPage, HistoryQuery, SessionStart};
use crate::backup::{BackupInfo, BackupService, BackupStatus, OpenableFolder, RelocateOutcome};
use crate::bank::catalog::{self, CollectionDetail, CollectionPage, CollectionQuery, TopicTree};
use crate::bank::{self, TopicPath};
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

const APP_NAME: &str = "Gabaritei";

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
        name: APP_NAME.to_owned(),
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

#[tauri::command]
pub fn question_count_limits(
    context: crate::question_count::CountContext,
) -> AppResult<crate::question_count::CountLimits> {
    Ok(context.limits())
}

/// Counts and facets for what Banco has checked. Numbers only, never question text.
#[tauri::command(async)]
pub fn selection_summary(
    db: State<'_, Database>,
    filter: crate::selection::QuestionFilter,
) -> AppResult<crate::selection::SelectionSummary> {
    db.with(|conn| crate::selection::summary(conn, &filter, &crate::clock::now()))
}

/// Creates a topic ahead of time and returns it with the spelling it was stored under.
#[tauri::command(async)]
pub fn create_topic(db: State<'_, Database>, path: TopicPath) -> AppResult<TopicPath> {
    db.with(|conn| bank::create_topic(conn, &path))
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
    let Some(source) = pick_folder(
        &app,
        "Escolha a pasta do Caderno Local (repositório Simulados-Concursos)",
    )?
    else {
        return Ok(None);
    };
    run_blocking(app, move |app| {
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
pub fn data_folder(paths: State<'_, AppPaths>) -> AppResult<String> {
    Ok(paths.data_dir().display().to_string())
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
    service.backup_now(&db, &paths, time::OffsetDateTime::now_utc())
}

/// Asks for a new backup folder and saves it. `None` if the user cancels.
#[tauri::command]
pub async fn backup_choose_folder<R: Runtime>(
    app: AppHandle<R>,
) -> AppResult<Option<BackupStatus>> {
    let Some(chosen) = pick_folder(&app, "Escolha a pasta dos backups")? else {
        return Ok(None);
    };
    run_blocking(app, move |app| {
        let db = app.state::<Database>();
        let paths = app.state::<AppPaths>();
        let service = app.state::<BackupService>();
        service.change_folder(&db, &paths, Some(&chosen)).map(Some)
    })
    .await
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
    let mode = if remove_originals {
        crate::backup::Relocation::Move
    } else {
        crate::backup::Relocation::Copy
    };
    service.relocate_left_behind(&db, &paths, mode)
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
    jobs::replace_study_data(
        &service,
        &db,
        &paths,
        &generation,
        &on_demand,
        DataReplacement::Restore(&name),
        time::OffsetDateTime::now_utc(),
    )
}

/// Asks where to save the data file and writes it. `false` if the user cancels.
#[tauri::command]
pub async fn export_data<R: Runtime>(app: AppHandle<R>) -> AppResult<bool> {
    run_blocking(app, move |app| {
        let now = crate::clock::now();
        let Some(file) = app
            .dialog()
            .file()
            .set_title("Exportar dados")
            .set_file_name(crate::backup::data_file_name(&now))
            .add_filter("Dados do Gabaritei", &[crate::backup::DATA_FILE_EXTENSION])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let dest = local_path(file, "Escolha um local deste computador.")?;
        let db = app.state::<Database>();
        let paths = app.state::<AppPaths>();
        let service = app.state::<BackupService>();
        service.export(&db, &paths, &dest, &now).map(|()| true)
    })
    .await
}

/// Asks for a data file and replaces the local data with it, after a safety backup. `false` if
/// the user cancels. Not while a generation runs.
#[tauri::command]
pub async fn import_data<R: Runtime>(app: AppHandle<R>) -> AppResult<bool> {
    run_blocking(app, move |app| {
        let Some(file) = app
            .dialog()
            .file()
            .set_title("Importar dados")
            .add_filter("Dados do Gabaritei", &[crate::backup::DATA_FILE_EXTENSION])
            .blocking_pick_file()
        else {
            return Ok(false);
        };
        let source = local_path(file, "Escolha um arquivo deste computador.")?;
        let db = app.state::<Database>();
        let paths = app.state::<AppPaths>();
        let service = app.state::<BackupService>();
        let generation = app.state::<GenerationManager>();
        let on_demand = app.state::<OnDemand>();
        jobs::replace_study_data(
            &service,
            &db,
            &paths,
            &generation,
            &on_demand,
            DataReplacement::Import(&source),
            time::OffsetDateTime::now_utc(),
        )
        .map(|()| true)
    })
    .await
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

/// Unfinished attempts, newest first, for the *Em andamento* list of Histórico.
#[tauri::command(async)]
pub fn active_attempts(db: State<'_, Database>) -> AppResult<Vec<ActiveAttempt>> {
    db.with(|conn| attempts::active(conn))
}

/// One page of finished attempts with scores from the current keys. No questions or keys.
#[tauri::command(async)]
pub fn attempt_history(db: State<'_, Database>, query: HistoryQuery) -> AppResult<HistoryPage> {
    db.with(|conn| attempts::history(conn, &query))
}

/// The *Erros e revisões* page: counts, and the list of reviews and errors. No answer keys.
#[tauri::command(async)]
pub fn learning_overview(
    db: State<'_, Database>,
    include_blanks: bool,
) -> AppResult<LearningOverview> {
    db.with(|conn| learning::overview(conn, &crate::clock::now(), include_blanks.into()))
}

/// Asks where to save the performance table as CSV (using the same filters) and writes it.
/// Returns `false` if the user cancels.
#[tauri::command]
pub async fn export_performance_csv<R: Runtime>(
    app: AppHandle<R>,
    query: PerformanceQuery,
) -> AppResult<bool> {
    let performance = app
        .state::<Database>()
        .with(|conn| metrics::performance(conn, &query))?;
    run_blocking(app, move |app| {
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
        let path = local_path(file, "Escolha um local deste computador.")?;
        metrics::export_csv(&path, &performance)?;
        Ok(true)
    })
    .await
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

/// Catalog choices and validation for the role being edited, without loading the CLI.
#[tauri::command]
pub fn ai_role_options(
    db: State<'_, Database>,
    cache: State<'_, CatalogCache>,
    config: RoleConfig,
) -> AppResult<model_catalog::RoleOptions> {
    let cli_paths = db.with(|conn| settings::load_cli_paths(conn))?;
    Ok(cache.role_options(&cli_paths, &config))
}

/// Changes the agent or model and reconciles its effort and Fast Mode.
#[tauri::command]
pub fn ai_change_role(
    db: State<'_, Database>,
    cache: State<'_, CatalogCache>,
    config: RoleConfig,
    change: model_catalog::RoleChange,
) -> AppResult<RoleConfig> {
    let cli_paths = db.with(|conn| settings::load_cli_paths(conn))?;
    Ok(cache.change_role(&cli_paths, &config, &change))
}

/// Validates and saves the AI settings, and returns them as saved.
#[tauri::command]
pub fn save_ai_settings(
    db: State<'_, Database>,
    secrets: State<'_, Secrets>,
    catalog: State<'_, CatalogCache>,
    input: AiSettingsInput,
) -> AppResult<AiSettings> {
    settings::save_and_reload(&db, &secrets, &catalog, input)
}

/// Saves the RemNote token in the OS keychain; `None` or blank removes it. Returns whether a
/// token is saved. The token is never returned.
#[tauri::command]
pub fn set_remnote_token(secrets: State<'_, Secrets>, token: Option<String>) -> AppResult<bool> {
    secrets.set_remnote_token(token.as_deref())
}

/// Finds the CLI (using the saved path override) and reports its version and login state.
#[tauri::command(async)]
pub fn cli_test(
    db: State<'_, Database>,
    secrets: State<'_, Secrets>,
    agent: Agent,
) -> AppResult<CliStatus> {
    settings::test_cli(&db, &secrets, agent)
}

/// Checks the saved RemNote connection and lists the tools the server offers.
#[tauri::command(async)]
pub fn remnote_test(
    db: State<'_, Database>,
    secrets: State<'_, Secrets>,
) -> AppResult<RemNoteTest> {
    let settings = db.with(|conn| settings::load(conn, &secrets))?;
    remnote_probe::remnote_test(&settings, &secrets)
}

/// Name of the window event that carries every [`GenerationStatus`] change.
pub const GENERATION_EVENT: &str = "generation-status";

fn resolve_job_clis(db: &Database) -> AppResult<ResolvedClis> {
    let overrides = db.with(|conn| settings::load_cli_paths(conn))?;
    let resolve = |agent| discovery::find_cli(agent, overrides.get(agent).map(Path::new));
    Ok(ResolvedClis::new(
        resolve(Agent::Claude),
        resolve(Agent::Codex),
    ))
}

fn with_generation_runtime<T>(
    clis: &ResolvedClis,
    work: impl FnOnce(&GenerationRuntime<'_>) -> AppResult<T>,
) -> AppResult<T> {
    let origin = Instant::now();
    work(&GenerationRuntime {
        clis,
        now: &crate::clock::now,
        monotonic_time: &|| origin.elapsed(),
    })
}

/// Starts a generation in the background. Fails at once if the form is invalid or another
/// generation is running. Progress and the result arrive as [`GENERATION_EVENT`] events.
#[tauri::command(async)]
pub fn generation_start<R: Runtime>(
    app: AppHandle<R>,
    manager: State<'_, GenerationManager>,
    service: State<'_, BackupService>,
    paths: State<'_, AppPaths>,
    mut request: GenerateRequest,
) -> AppResult<()> {
    service.ensure_idle()?;
    roles::validate_generation(
        &app.state::<Database>(),
        &app.state::<CatalogCache>(),
        &mut request,
    )?;
    let clis = resolve_job_clis(&app.state::<Database>())?;
    let files_dir = paths.draft_files_dir();
    let worker = app.clone();
    manager.start(
        GenerationKind::Generate,
        crate::clock::now(),
        move |cancel, progress| {
            let db = worker.state::<Database>();
            let secrets = worker.state::<Secrets>();
            with_generation_runtime(&clis, |runtime| {
                blind_check::generate_and_check(
                    &db, &secrets, runtime, &files_dir, request, cancel, progress,
                )
            })
        },
        move |status| emit_generation_status(&app, status),
    )
}

/// Runs the blind check again on a draft whose check failed. Uses the same background slot as
/// a generation, so only one runs at a time.
#[tauri::command(async)]
pub fn blind_check_start<R: Runtime>(
    app: AppHandle<R>,
    manager: State<'_, GenerationManager>,
    service: State<'_, BackupService>,
    paths: State<'_, AppPaths>,
    draft_id: DraftId,
) -> AppResult<()> {
    service.ensure_idle()?;
    roles::validate_saved_role(
        &app.state::<Database>(),
        &app.state::<CatalogCache>(),
        Role::BlindCheck,
    )?;
    let clis = resolve_job_clis(&app.state::<Database>())?;
    let files_dir = paths.draft_files_dir();
    let worker = app.clone();
    manager.start(
        GenerationKind::Recheck,
        crate::clock::now(),
        move |cancel, progress| {
            let db = worker.state::<Database>();
            let secrets = worker.state::<Secrets>();
            with_generation_runtime(&clis, |runtime| {
                blind_check::recheck_draft(
                    &db, &secrets, runtime, &files_dir, &draft_id, cancel, progress,
                )
            })
        },
        move |status| emit_generation_status(&app, status),
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
    review::import(
        &db,
        &paths.draft_files_dir(),
        &id,
        mode,
        &crate::clock::now(),
    )
}

/// Imports every question, running the blind check first when needed.
#[tauri::command]
pub async fn draft_import_all<R: Runtime>(
    app: AppHandle<R>,
    id: DraftId,
) -> AppResult<ImportAllOutcome> {
    run_blocking(app, move |app| {
        let db = app.state::<Database>();
        let files_dir = app.state::<AppPaths>().draft_files_dir();
        review::import_all(
            &db,
            &files_dir,
            &id,
            &app.state::<GenerationManager>(),
            crate::clock::now,
            |cancel, progress| {
                app.state::<BackupService>().ensure_idle()?;
                roles::validate_saved_role(&db, &app.state::<CatalogCache>(), Role::BlindCheck)?;
                let secrets = app.state::<Secrets>();
                let clis = resolve_job_clis(&db)?;
                with_generation_runtime(&clis, |runtime| {
                    blind_check::recheck_draft(
                        &db, &secrets, runtime, &files_dir, &id, cancel, progress,
                    )
                })?;
                Ok(())
            },
            |status| emit_generation_status(&app, status),
        )
    })
    .await
}

/// Discards a draft that is waiting for review.
#[tauri::command]
pub fn draft_discard(
    db: State<'_, Database>,
    paths: State<'_, AppPaths>,
    id: DraftId,
) -> AppResult<()> {
    review::discard(&db, &paths.draft_files_dir(), &id)
}

/// Asks which PDF, Markdown or text files to attach to a generation. Empty if cancelled.
#[tauri::command]
pub async fn pick_attachments<R: Runtime>(app: AppHandle<R>) -> AppResult<Vec<String>> {
    run_blocking(app, move |app| {
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
                local_path(file, "Escolha arquivos deste computador.")
                    .map(|path| path.to_string_lossy().into_owned())
            })
            .collect()
    })
    .await
}

/// Asks the re-check role whether the key of a revealed question is right. Waits for the AI;
/// the proposal stays in the backend until it is applied or dismissed.
#[tauri::command]
pub async fn recheck_ask<R: Runtime>(
    app: AppHandle<R>,
    attempt_id: AttemptId,
    position: u32,
) -> AppResult<RecheckView> {
    run_blocking(app, move |app| {
        let db = app.state::<Database>();
        let secrets = app.state::<Secrets>();
        let jobs = app.state::<OnDemand>();
        let catalog = app.state::<CatalogCache>();
        roles::validate_saved_role(&db, &catalog, Role::Recheck)?;
        let clis = resolve_job_clis(&db)?;
        jobs.run(|cancel| {
            recheck::ask_recheck(&db, &secrets, &clis, &jobs, &attempt_id, position, cancel)
        })
    })
    .await
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
    run_blocking(app, move |app| {
        let db = app.state::<Database>();
        let secrets = app.state::<Secrets>();
        let jobs = app.state::<OnDemand>();
        let catalog = app.state::<CatalogCache>();
        roles::validate_saved_role(&db, &catalog, Role::Explain)?;
        let clis = resolve_job_clis(&db)?;
        jobs.run(|cancel| {
            explain::ask_explain(
                &db,
                &secrets,
                &clis,
                &attempt_id,
                position,
                cancel,
                crate::clock::now,
            )
        })
    })
    .await
}

/// Cancels the on-demand AI request that is running (re-check or explain).
#[tauri::command]
pub fn ai_request_cancel(jobs: State<'_, OnDemand>) -> AppResult<bool> {
    Ok(jobs.cancel())
}

/// Stops the running generation. Returns whether one was running.
#[tauri::command]
pub fn generation_cancel(manager: State<'_, GenerationManager>) -> AppResult<bool> {
    Ok(manager.cancel())
}

/// What the generation is doing now, or how the last one ended.
#[tauri::command]
pub fn generation_status(manager: State<'_, GenerationManager>) -> AppResult<GenerationStatus> {
    Ok(manager.status())
}

/// Forgets how the last generation ended.
#[tauri::command]
pub fn generation_dismiss(manager: State<'_, GenerationManager>) -> AppResult<()> {
    manager.dismiss();
    Ok(())
}

async fn run_blocking<R: Runtime, T: Send + 'static>(
    app: AppHandle<R>,
    work: impl FnOnce(AppHandle<R>) -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(move || work(app))
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?
}

fn pick_folder<R: Runtime>(app: &AppHandle<R>, title: &str) -> AppResult<Option<PathBuf>> {
    app.dialog()
        .file()
        .set_title(title)
        .blocking_pick_folder()
        .map(|folder| local_path(folder, "Escolha uma pasta deste computador."))
        .transpose()
}

fn local_path(file: FilePath, message: &str) -> AppResult<PathBuf> {
    file.into_path()
        .map_err(|_| AppError::Invalid(message.into()))
}

fn emit_generation_status<R: Runtime>(app: &AppHandle<R>, status: &GenerationStatus) {
    if let Err(error) = app.emit(GENERATION_EVENT, status) {
        crate::log::error(format!("could not emit generation status: {error}"));
    }
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
    fn collections_decodes_camel_case_arguments() {
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
    }

    #[test]
    fn collection_reports_an_unknown_id() {
        let missing = invoke_with(
            "collection",
            json!({ "id": "nope" }),
            Database::open_in_memory().unwrap(),
        );
        assert_eq!(
            missing,
            Err(json!({ "message": "Caderno não encontrado." }))
        );
    }

    #[test]
    fn topics_tree_returns_an_empty_tree_for_an_empty_bank() {
        assert_eq!(
            invoke("topics_tree"),
            Ok(json!({ "areas": [], "questions": 0 }))
        );
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
                input.explanation = format!("SEGREDO-{}", input.id);
                input.refs = vec![format!("REF-{}", input.id)];
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
    fn backup_status_lists_the_default_folder_when_empty() {
        let status = invoke("backup_status").expect("status");
        assert_eq!(status["isDefaultFolder"], json!(true));
        assert_eq!(
            status["listing"],
            json!({ "state": "ready", "backups": [] })
        );
    }

    #[test]
    fn backup_now_appears_in_the_listing_as_a_manual_backup() {
        let app = MockApp::new(Database::open_in_memory().unwrap());
        // The in-memory database has no file to back up from, but the online backup copies it.
        let made = app.call("backup_now", json!({})).expect("backs up");
        assert_eq!(made["kind"], json!("manual"));

        let status = app.call("backup_status", json!({})).expect("status");
        assert_eq!(status["listing"]["backups"], json!([made]));
        assert_eq!(status["lastError"], json!(null));
    }

    #[test]
    fn backup_restore_makes_a_safety_backup() {
        let app = MockApp::new(Database::open_in_memory().unwrap());
        let made = app.call("backup_now", json!({})).expect("backs up");

        app.call("backup_restore", json!({ "name": made["name"] }))
            .expect("restores");

        let status = app.call("backup_status", json!({})).expect("status");
        let backups = status["listing"]["backups"].as_array().unwrap();
        assert_eq!(backups.len(), 2);
        assert_eq!(backups[0]["kind"], json!("safety"));
    }

    #[test]
    fn backup_restore_refuses_a_name_that_escapes_the_backup_folder() {
        let app = MockApp::new(Database::open_in_memory().unwrap());
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
    fn ai_settings_returns_the_defaults() {
        let settings = invoke("ai_settings").expect("loads");
        assert_eq!(settings["roles"]["generate"]["model"], json!("gpt-6.2-sol"));
        assert_eq!(settings["roles"]["generate"]["agent"], json!("codex"));
        assert_eq!(settings["roles"]["blindCheck"]["effort"], json!("xhigh"));
        assert_eq!(settings["hasRemnoteToken"], json!(false));
    }

    #[test]
    fn ai_settings_saves_changes_to_roles_and_remnote() {
        let app = MockApp::new(Database::open_in_memory().unwrap());
        let mut input = app.call("ai_settings", json!({})).expect("loads");
        input["remnote"] = json!({
            "mode": "server", "url": "https://r.example/mcp", "tools": ["search"]
        });
        input["roles"]["explain"] = json!({
            "agent": "claude", "model": "claude-sonnet-5-5", "effort": "medium", "fast": false
        });

        let after = app
            .call("save_ai_settings", json!({ "input": input }))
            .expect("saves");
        assert_eq!(after["roles"]["explain"], input["roles"]["explain"]);
        assert_eq!(after["remnote"], input["remnote"]);
        assert_eq!(app.call("ai_settings", json!({})).expect("reloads"), after);
    }

    #[test]
    fn ai_settings_responses_only_show_whether_the_token_is_saved() {
        let app = MockApp::new(Database::open_in_memory().unwrap());
        let saved = app
            .call("set_remnote_token", json!({ "token": "TOKEN-SECRETO" }))
            .expect("token saves");
        assert_eq!(saved, json!(true));

        let loaded = app.call("ai_settings", json!({})).expect("loads");
        let after = app
            .call("save_ai_settings", json!({ "input": loaded }))
            .expect("saves");
        for response in [loaded, after] {
            assert_eq!(response["hasRemnoteToken"], json!(true));
            assert!(!response.to_string().contains("TOKEN-SECRETO"));
        }
    }

    #[test]
    fn set_remnote_token_clears_the_saved_token_for_blank_input() {
        let app = MockApp::new(Database::open_in_memory().unwrap());
        app.call("set_remnote_token", json!({ "token": "TOKEN-SECRETO" }))
            .unwrap();

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

    #[test]
    fn a_failed_ai_settings_write_preserves_the_saved_values() {
        let db = Database::open_in_memory().unwrap();
        db.with(|conn| {
            conn.execute_batch(
                "CREATE TRIGGER refuse_remnote_settings BEFORE INSERT ON settings
                 WHEN NEW.key = 'ai.remnote' BEGIN
                    SELECT RAISE(ABORT, 'test write failure');
                 END;",
            )?;
            Ok(())
        })
        .unwrap();
        let app = MockApp::new(db);
        let before = app.call("ai_settings", json!({})).unwrap();
        let mut input = before.clone();
        input["roles"]["generate"]["model"] = json!("changed");

        let result = app.call("save_ai_settings", json!({ "input": input }));

        assert_eq!(
            result,
            Err(json!({ "message": "Ocorreu um erro inesperado. Tente novamente." }))
        );
        assert_eq!(app.call("ai_settings", json!({})), Ok(before));
    }

    #[cfg(unix)]
    struct CheckedDraft {
        app: MockApp,
        cli: tempfile::TempDir,
        status: Value,
        id: String,
    }

    #[cfg(unix)]
    fn generate_checked_draft() -> CheckedDraft {
        use std::sync::mpsc;
        use std::time::Duration;
        use tauri::Listener;

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
        std::fs::write(dir.path().join("answer-3.json"), verdicts.to_string()).unwrap();
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
        let (finished, done) = mpsc::channel();
        let listener = app._app.listen(GENERATION_EVENT, move |event| {
            let status: Value = serde_json::from_str(event.payload()).expect("generation status");
            if status["running"] == json!(false) {
                finished
                    .send(status)
                    .expect("subscriber waits for completion");
            }
        });
        app.call("generation_start", json!({ "request": request }))
            .expect("starts");
        let status = done
            .recv_timeout(Duration::from_secs(10))
            .expect("completion event");
        app._app.unlisten(listener);
        assert_eq!(status["outcome"]["kind"], json!("draft"), "{status}");
        let id = status["outcome"]["draftId"].as_str().unwrap().to_owned();
        CheckedDraft {
            app,
            cli: dir,
            status,
            id,
        }
    }

    #[cfg(unix)]
    #[test]
    fn generation_reports_a_finished_draft_without_question_content() {
        let CheckedDraft {
            app,
            cli,
            status,
            id,
        } = generate_checked_draft();
        assert!(id.len() > 8);
        assert_eq!(
            std::fs::read_to_string(cli.path().join("counter"))
                .unwrap()
                .trim(),
            "2"
        );
        assert_eq!(app.call("generation_status", json!({})).unwrap(), status);
        assert!(!status.to_string().contains("crase"));
    }

    #[cfg(unix)]
    #[test]
    fn draft_summary_shows_the_check_state_without_agreed_question_content() {
        let CheckedDraft { app, id, .. } = generate_checked_draft();
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
    }

    #[cfg(unix)]
    #[test]
    fn drafts_waiting_lists_the_draft_without_question_content() {
        let CheckedDraft { app, id, .. } = generate_checked_draft();
        let waiting = app.call("drafts_waiting", json!({})).unwrap();
        assert_eq!(waiting[0]["id"], json!(id));
        assert!(!waiting.to_string().contains("crase"));
    }

    #[cfg(unix)]
    #[test]
    fn draft_import_creates_a_generated_collection_and_closes_the_draft() {
        let CheckedDraft { app, id, .. } = generate_checked_draft();
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
    }

    #[cfg(unix)]
    #[test]
    fn draft_import_all_checks_an_unfinished_draft_and_imports_it_in_one_call() {
        let CheckedDraft { app, cli, id, .. } = generate_checked_draft();
        app._app
            .state::<Database>()
            .with(|conn| {
                conn.execute("UPDATE drafts SET blind_check = NULL WHERE id = ?1", [&id])?;
                Ok(())
            })
            .unwrap();
        let summary = app.call("draft_summary", json!({ "id": id })).unwrap();
        assert_eq!(summary["needsCheck"], json!(true));

        let outcome = app.call("draft_import_all", json!({ "id": id })).unwrap();

        assert_eq!(outcome["kind"], json!("imported"));
        assert_eq!(outcome["result"]["imported"], json!(1));
        assert_eq!(
            std::fs::read_to_string(cli.path().join("counter"))
                .unwrap()
                .trim(),
            "3"
        );
        assert_eq!(app.call("drafts_waiting", json!({})).unwrap(), json!([]));
        let status = app.call("generation_status", json!({})).unwrap();
        assert_eq!(status["running"], json!(false));
        assert_eq!(status["outcome"], Value::Null);
    }

    #[cfg(unix)]
    #[test]
    fn generation_dismiss_clears_the_finished_status() {
        let CheckedDraft { app, .. } = generate_checked_draft();
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
    fn changing_a_cli_path_makes_the_next_catalog_read_load_again() {
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
            "cliPaths": { "codex": "/opt/new-codex", "claude": null },
            "roles": settings["roles"], "remnote": settings["remnote"],
        });
        app.call("save_ai_settings", json!({ "input": input }))
            .unwrap();

        app.call("ai_catalog", json!({ "refresh": false })).unwrap();
        assert_eq!(loads.load(Ordering::SeqCst), before + 2);
    }

    #[test]
    fn a_changed_cli_path_accepts_a_model_absent_from_the_previous_list() {
        let cache = CatalogCache::new(vec![Box::new(OneModel::default())]);
        cache.get(&settings::CliPaths::default());
        let app = MockApp::with_catalog(Database::open_in_memory().unwrap(), cache);
        let mut input = app.call("ai_settings", json!({})).unwrap();
        input["cliPaths"]["codex"] = json!("/opt/new-codex");
        input["roles"]["generate"]["model"] = json!("new-model");
        assert_eq!(
            app.call("save_ai_settings", json!({ "input": input.clone() }))
                .unwrap(),
            input,
        );
    }

    #[test]
    fn saving_a_new_unlisted_model_leaves_the_roles_unchanged() {
        let cache = CatalogCache::new(vec![Box::new(OneModel::default())]);
        cache.get(&settings::CliPaths::default());
        let app = MockApp::with_catalog(Database::open_in_memory().unwrap(), cache);
        let saved = app.call("ai_settings", json!({})).unwrap();
        let mut edited = saved.clone();
        edited["roles"]["generate"]["model"] = json!("obsolete");

        let problem = app
            .call("save_ai_settings", json!({ "input": edited }))
            .unwrap_err();
        assert!(problem["message"].as_str().unwrap().contains("obsolete"));
        assert_eq!(app.call("ai_settings", json!({})).unwrap(), saved);
    }

    #[test]
    fn an_unchanged_stale_role_allows_an_unrelated_settings_save() {
        let cache = CatalogCache::new(vec![Box::new(OneModel::default())]);
        cache.get(&settings::CliPaths::default());
        let app = MockApp::with_catalog(Database::open_in_memory().unwrap(), cache);
        let mut input = app.call("ai_settings", json!({})).unwrap();
        input["remnote"] = json!({ "mode": "server", "url": "https://r.example/mcp", "tools": [] });
        assert_eq!(
            app.call("save_ai_settings", json!({ "input": input.clone() }))
                .unwrap(),
            input,
        );
    }

    #[test]
    fn editing_a_role_gets_the_catalogs_options_and_problem() {
        let cache = CatalogCache::new(vec![Box::new(OneModel::default())]);
        cache.get(&settings::CliPaths::default());
        let app = MockApp::with_catalog(Database::open_in_memory().unwrap(), cache);
        let config = json!({ "agent": "codex", "model": "m1", "effort": "high", "fast": false });
        assert_eq!(
            app.call("ai_role_options", json!({ "config": config }))
                .unwrap(),
            json!({
                "efforts": ["low"], "showFast": false,
                "problem": "O esforço “high” não existe para o modelo “m1”. Escolha outro."
            })
        );
    }

    #[test]
    fn switching_agents_uses_the_backend_role_transition() {
        let cache = CatalogCache::new(vec![Box::new(OneModel::default())]);
        cache.get(&settings::CliPaths::default());
        let app = MockApp::with_catalog(Database::open_in_memory().unwrap(), cache);
        assert_eq!(
            app.call("ai_change_role", json!({
                "config": { "agent": "claude", "model": "sonnet", "effort": "high", "fast": true },
                "change": { "kind": "agent", "agent": "codex" }
            })).unwrap(),
            json!({ "agent": "codex", "model": "m1", "effort": "low", "fast": false }),
        );
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
