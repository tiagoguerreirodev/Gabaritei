//! Gabaritei backend.
//!
//! All business logic lives here, in plain Rust modules that can be tested without Tauri.
//! Tauri commands (in [`commands`]) only decode input, call a domain function and encode
//! the output. See `AGENTS.md` → "Architecture rules".

pub mod ai;
pub mod attempts;
pub mod backup;
pub mod bank;
pub mod clock;
pub mod commands;
pub mod db;
pub mod error;
pub mod images;
pub mod learning;
pub mod log;
pub mod metrics;
pub mod migrate;
pub mod paths;

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
            commands::collections,
            commands::collection,
            commands::legacy_import_status,
            commands::import_legacy_bank,
            commands::home,
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
            commands::save_ai_settings,
            commands::set_remnote_token,
            commands::cli_test,
            commands::remnote_test,
            commands::generation_start,
            commands::blind_check_start,
            commands::drafts_waiting,
            commands::draft_summary,
            commands::draft_import,
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
    let opened = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::Internal(error.to_string()))
        .and_then(|data_dir| {
            log::init(&data_dir);
            open_state(data_dir)
        });
    let (database, paths) = match opened {
        Ok(state) => state,
        Err(error) => {
            log::error(format!("could not start: {error} ({error:?})"));
            // The main window already exists; hide it so only the dialog shows. The dialog must not
            // block here: this hook runs on the main thread, which the dialog needs to be drawn
            // on Linux. Closing it ends the app through Tauri's own exit.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.hide();
            }
            let handle = app.handle().clone();
            app.dialog()
                .message(startup_message(&error))
                .title("Gabaritei")
                .kind(MessageDialogKind::Error)
                .show(move |_| handle.exit(1));
            return Ok(());
        }
    };
    app.manage(database);
    app.manage(paths);
    app.manage(backup::BackupService::default());
    start_backup_schedule(app.handle().clone());
    app.manage(ai::secrets::Secrets::keychain());
    app.manage(ai::jobs::GenerationManager::default());
    app.manage(ai::ondemand::OnDemand::default());
    Ok(())
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
            let mut retry_at: Option<std::time::Instant> = None;
            loop {
                let waiting = retry_at.is_some_and(|at| std::time::Instant::now() < at);
                if let (false, Some(db), Some(paths), Some(service)) = (
                    waiting,
                    app.try_state::<db::Database>(),
                    app.try_state::<paths::AppPaths>(),
                    app.try_state::<backup::BackupService>(),
                ) {
                    let failed = run_scheduled_backup(
                        &service,
                        &db,
                        &paths,
                        time::OffsetDateTime::now_utc(),
                    );
                    retry_at = failed.then(|| std::time::Instant::now() + BACKUP_RETRY);
                }
                std::thread::sleep(BACKUP_POLL);
            }
        });
    if let Err(error) = spawned {
        log::error(format!("could not start the backup schedule: {error}"));
    }
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
                db::set_setting(conn, "backup.folder", &file.join("sub").to_str().unwrap())
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
