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
pub mod metrics;
pub mod migrate;
pub mod paths;

use tauri::{App, Builder, Manager, Runtime};

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
            commands::topics_tree,
            commands::collections,
            commands::collection,
            commands::legacy_import_status,
            commands::import_legacy_bank,
            commands::home,
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

/// Opens the database in the OS app data directory and makes it and the app's paths
/// available to commands.
fn open_database<R: Runtime>(app: &mut App<R>) -> Result<(), Box<dyn std::error::Error>> {
    let paths = paths::AppPaths::new(app.path().app_data_dir()?);
    let database = db::Database::open(&paths.database()).inspect_err(|error| {
        eprintln!("Gabaritei: {error} ({:?}: {error:?})", paths.database());
    })?;
    database.with(|conn| ai::blind_check::recover_interrupted(conn))?;
    app.manage(database);
    app.manage(paths);
    app.manage(ai::secrets::Secrets::keychain());
    app.manage(ai::jobs::GenerationManager::default());
    app.manage(ai::ondemand::OnDemand::default());
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    register(tauri::Builder::default())
        .plugin(tauri_plugin_dialog::init())
        .setup(open_database)
        .run(tauri::generate_context!())
        .expect("error while running the Gabaritei application");
}
