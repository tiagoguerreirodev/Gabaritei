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
pub mod learning;
pub mod metrics;
pub mod migrate;
pub mod paths;

use tauri::{App, Builder, Manager, Runtime};

/// Registers every command on a Tauri builder.
///
/// Shared by [`run`] and by the IPC tests, which use Tauri's mock runtime.
pub fn register<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder.invoke_handler(tauri::generate_handler![
        commands::app_info,
        commands::legacy_import_status,
        commands::import_legacy_bank,
    ])
}

/// Opens the database in the OS app data directory and makes it and the app's paths
/// available to commands.
fn open_database<R: Runtime>(app: &mut App<R>) -> Result<(), Box<dyn std::error::Error>> {
    let paths = paths::AppPaths::new(app.path().app_data_dir()?);
    let database = db::Database::open(&paths.database()).inspect_err(|error| {
        eprintln!("Gabaritei: {error} ({:?}: {error:?})", paths.database());
    })?;
    app.manage(database);
    app.manage(paths);
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
