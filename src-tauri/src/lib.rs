//! Gabaritei backend.
//!
//! All business logic lives here, in plain Rust modules that can be tested without Tauri.
//! Tauri commands (in [`commands`]) only decode input, call a domain function and encode
//! the output. See `AGENTS.md` → "Architecture rules".

pub mod ai;
pub mod appearance;
pub mod attempts;
pub mod backup;
pub mod bank;
pub mod clock;
pub mod commands;
pub mod db;
mod enum_text;
pub mod error;
pub mod ids;
pub mod images;
pub mod learning;
pub mod log;
pub mod metrics;
pub mod migrate;
mod ordering;
pub mod paths;
mod question_count;
pub mod selection;
pub mod shortcuts;
mod sync;

use std::path::PathBuf;

use tauri::{App, Builder, Manager, Runtime};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

use crate::error::{AppError, AppResult};

/// Registers every command on a Tauri builder.
///
/// Shared by [`run`] and by the IPC tests, which use Tauri's mock runtime.
pub fn register<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder
        .register_asynchronous_uri_scheme_protocol(images::SCHEME, |ctx, request, responder| {
            let images_dir = ctx
                .app_handle()
                .try_state::<paths::AppPaths>()
                .map(|paths| paths.images_dir());
            tauri::async_runtime::spawn_blocking(move || {
                let response = match images_dir {
                    Some(dir) => images::respond(&dir, &request),
                    // Without the app's folders there is nothing to serve.
                    None => images::not_found(),
                };
                responder.respond(response);
            });
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::appearance,
            commands::save_appearance,
            commands::keyboard_shortcuts,
            commands::save_keyboard_shortcuts,
            commands::backup_status,
            commands::data_folder,
            commands::open_folder,
            commands::backup_now,
            commands::backup_choose_folder,
            commands::backup_use_default_folder,
            commands::backup_relocate_left_behind,
            commands::backup_forget_left_behind,
            commands::backup_restore,
            commands::export_data,
            commands::import_data,
            commands::topics_tree,
            commands::selection_summary,
            commands::create_topic,
            commands::question_count_limits,
            commands::collections,
            commands::collection,
            commands::legacy_import_status,
            commands::import_legacy_bank,
            commands::home,
            commands::active_attempts,
            commands::attempt_history,
            commands::attempt_closing,
            commands::learning_overview,
            commands::performance,
            commands::export_performance_csv,
            commands::session_start,
            commands::attempt,
            commands::answer,
            commands::eliminate,
            commands::flag,
            commands::finish,
            commands::abandon,
            commands::annul,
            commands::unannul,
            commands::ai_settings,
            commands::ai_catalog,
            commands::ai_role_options,
            commands::ai_change_role,
            commands::save_ai_settings,
            commands::set_remnote_token,
            commands::cli_test,
            commands::remnote_test,
            commands::generation_start,
            commands::blind_check_start,
            commands::drafts_waiting,
            commands::draft_summary,
            commands::draft_import,
            commands::draft_import_all,
            commands::draft_discard,
            commands::pick_attachments,
            commands::recheck_ask,
            commands::recheck_apply,
            commands::recheck_dismiss,
            commands::explain_ask,
            commands::ai_request_cancel,
            commands::generation_cancel,
            commands::generation_status,
            commands::generation_dismiss,
        ])
}

/// Opens the database in `data_dir` and recovers anything a crash left half done.
fn open_state(data_dir: PathBuf) -> AppResult<(db::Database, paths::AppPaths)> {
    let paths = paths::AppPaths::new(data_dir);
    let database = db::Database::open(&paths.database())?;
    database.with(|conn| ai::blind_check::recover_interrupted(conn))?;
    // Attachment copies of drafts that are gone (a kill mid-generation, a draft removed another way).
    database.with(|conn| ai::draft_files::sweep(conn, &paths.draft_files_dir()))?;
    // Reviews are derived: history from before the feature, or a restored backup, gets its schedule.
    database.transaction(|tx| learning::refresh_all(tx))?;
    Ok((database, paths))
}

/// What the user reads when the app can't start. Technical detail stays in the log.
fn startup_message(error: &AppError) -> String {
    match error {
        AppError::Invalid(message) => message.clone(),
        AppError::Internal(_) => "Não foi possível abrir os dados do Gabaritei. Verifique se a \
             pasta de dados do aplicativo existe e pode ser gravada, e tente novamente."
            .to_owned(),
    }
}

/// Opens the database in the OS app data directory and makes it and the app's paths
/// available to commands. When that fails, tells the user in a native dialog and exits when it is closed.
fn open_database<R: Runtime>(app: &mut App<R>) -> Result<(), Box<dyn std::error::Error>> {
    match open_app_data(app) {
        Ok((database, paths)) => manage_services(app, database, paths),
        Err(error) => show_startup_error(app, &error),
    }
    Ok(())
}

fn open_app_data<R: Runtime>(app: &App<R>) -> AppResult<(db::Database, paths::AppPaths)> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::Internal(error.to_string()))?;
    log::init(&data_dir);
    open_state(data_dir)
}

fn show_startup_error<R: Runtime>(app: &App<R>, error: &AppError) {
    log::error(format!("could not start: {error} ({error:?})"));
    // Closing the non-blocking dialog exits the app, leaving the main thread free to draw it.
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    let handle = app.handle().clone();
    app.dialog()
        .message(startup_message(error))
        .title("Gabaritei")
        .kind(MessageDialogKind::Error)
        .show(move |_| handle.exit(1));
}

fn manage_services<R: Runtime>(app: &App<R>, database: db::Database, paths: paths::AppPaths) {
    app.manage(database);
    app.manage(paths);
    const FOLDER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
    const MANIFEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
    app.manage(backup::BackupService::new(
        backup_computer(),
        FOLDER_TIMEOUT,
        MANIFEST_TIMEOUT,
        cleanup_restored_data,
    ));
    start_backup_schedule(app.handle().clone());
    app.manage(ai::secrets::Secrets::keychain());
    app.manage(ai::catalog::CatalogCache::new(vec![
        Box::new(ai::catalog::claude::ClaudeSource),
        Box::new(ai::catalog::codex::CodexSource),
    ]));
    app.manage(ai::jobs::GenerationManager::default());
    app.manage(ai::ondemand::OnDemand::default());
}

fn cleanup_restored_data(conn: &rusqlite::Connection) -> AppResult<()> {
    ai::blind_check::recover_interrupted(conn)?;
    learning::refresh_all(conn)
}

/// Reads the machine names at startup, before the backup service starts working.
fn backup_computer() -> backup::Computer {
    let local = local_host_name();
    let host = gethostname::gethostname();
    backup::Computer::from_host_names(local.as_deref(), &host.to_string_lossy())
}

/// macOS keeps a stable local name apart from the network host name.
#[cfg(target_os = "macos")]
fn local_host_name() -> Option<String> {
    let output = std::process::Command::new("/usr/sbin/scutil")
        .args(["--get", "LocalHostName"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(not(target_os = "macos"))]
fn local_host_name() -> Option<String> {
    None
}

/// How often the schedule looks at the wall clock. Short, because a long sleep may not count
/// the time the computer was asleep on every OS; whether a backup is due is decided from the
/// clock and the newest backup, never from how many sleeps went by.
const BACKUP_POLL: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// After a failed backup the schedule waits this long before trying again, so a lasting
/// problem (a full disk, a missing drive) doesn't repeat the whole copy every few minutes.
const BACKUP_RETRY: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Makes the daily backup if one is due. A failure is kept in [`backup::BackupService`] for
/// Configurações → Dados, which only has the pt-BR message; the detail goes to the log.
/// Returns whether the attempt failed.
fn run_scheduled_backup(
    service: &backup::BackupService,
    db: &db::Database,
    paths: &paths::AppPaths,
    now: time::OffsetDateTime,
) -> bool {
    match service.backup_if_due(db, paths, now) {
        Ok(_) => false,
        Err(error) => {
            log::error(format!("the scheduled backup failed: {error:?}"));
            true
        }
    }
}

/// Checks for a due backup at start and then every few minutes while the app is open
/// (spec §3.6).
fn start_backup_schedule<R: Runtime>(app: tauri::AppHandle<R>) {
    let spawned = std::thread::Builder::new()
        .name("backup-schedule".into())
        .spawn(move || {
            let mut last_failure: Option<std::time::Instant> = None;
            loop {
                if ready_to_retry(last_failure.map(|time| time.elapsed()))
                    && let Some(failed) = poll_scheduled_backup(&app)
                {
                    last_failure = failed.then(std::time::Instant::now);
                }
                std::thread::sleep(BACKUP_POLL);
            }
        });
    if let Err(error) = spawned {
        log::error(format!("could not start the backup schedule: {error}"));
    }
}

fn ready_to_retry(time_since_failure: Option<std::time::Duration>) -> bool {
    time_since_failure.is_none_or(|elapsed| elapsed >= BACKUP_RETRY)
}

fn poll_scheduled_backup<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<bool> {
    let service = app.try_state::<backup::BackupService>()?;
    let db = app.try_state::<db::Database>()?;
    let paths = app.try_state::<paths::AppPaths>()?;
    Some(run_scheduled_backup(
        &service,
        &db,
        &paths,
        time::OffsetDateTime::now_utc(),
    ))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    register(tauri::Builder::default())
        .plugin(tauri_plugin_dialog::init())
        // Only used from Rust (`open_folder`); the page gets no click handler for links.
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        .setup(open_database)
        .run(tauri::generate_context!())
        .expect("error while running the Gabaritei application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_retry_waits_until_its_deadline() {
        assert!(ready_to_retry(None));
        assert!(!ready_to_retry(Some(
            BACKUP_RETRY - std::time::Duration::from_secs(1)
        )));
        assert!(ready_to_retry(Some(BACKUP_RETRY)));
    }

    #[test]
    fn a_newer_database_stops_startup_with_the_pt_br_message() {
        let dir = tempfile::tempdir().unwrap();
        let path = db::database_path(dir.path());
        drop(db::Database::open(&path).unwrap());
        rusqlite::Connection::open(&path)
            .unwrap()
            .pragma_update(None, "user_version", db::SCHEMA_VERSION + 1)
            .unwrap();

        let error = open_state(dir.path().to_path_buf())
            .err()
            .expect("a newer database is refused");
        assert_eq!(startup_message(&error), db::NEWER_SCHEMA_MESSAGE);
    }

    #[test]
    fn an_unwritable_data_folder_gets_a_generic_message_without_technical_detail() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-folder");
        std::fs::write(&file, "x").unwrap();

        let error = open_state(file.join("data"))
            .err()
            .expect("a data folder under a file cannot be created");
        assert!(matches!(error, AppError::Internal(_)));
        let message = startup_message(&error);
        assert!(message.contains("pasta de dados"));
    }

    fn scheduler_fixture() -> (tempfile::TempDir, paths::AppPaths, db::Database) {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths::AppPaths::new(dir.path().join("data"));
        let database = db::Database::open(&paths.database()).unwrap();
        (dir, paths, database)
    }

    #[test]
    fn the_scheduled_backup_follows_the_wall_clock_not_the_number_of_polls() {
        use time::macros::datetime;
        let (_dir, paths, database) = scheduler_fixture();
        let service = backup::BackupService::default();
        let count = || backup::list(&backup::default_folder(&paths)).len();

        run_scheduled_backup(&service, &database, &paths, datetime!(2026-11-19 10:00 UTC));
        assert_eq!(count(), 1);
        // A poll a few minutes later finds nothing due.
        run_scheduled_backup(&service, &database, &paths, datetime!(2026-11-19 10:05 UTC));
        assert_eq!(count(), 1);
        // The computer slept for a day: the very next poll makes the backup.
        run_scheduled_backup(&service, &database, &paths, datetime!(2026-11-20 10:06 UTC));
        assert_eq!(count(), 2);
    }

    #[test]
    fn a_failed_scheduled_backup_is_kept_for_the_data_page() {
        let (dir, paths, database) = scheduler_fixture();
        let file = dir.path().join("not-a-folder");
        std::fs::write(&file, "x").unwrap();
        database
            .with(|conn| {
                db::set_setting(
                    conn,
                    db::SettingKey::BackupFolder,
                    &file.join("sub").to_str().unwrap(),
                )
            })
            .unwrap();
        let service = backup::BackupService::default();

        let failed = run_scheduled_backup(
            &service,
            &database,
            &paths,
            time::macros::datetime!(2026-11-19 10:00 UTC),
        );

        // The schedule is told, so it can wait before the next try.
        assert!(failed);
        let status = service.status(&database, &paths).unwrap();
        assert!(status.last_error.is_some());
    }
}
