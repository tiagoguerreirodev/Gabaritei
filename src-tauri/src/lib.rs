//! Gabaritei backend.
//!
//! All business logic lives here, in plain Rust modules that can be tested without Tauri.
//! Tauri commands (in [`commands`]) only decode input, call a domain function and encode
//! the output. See `AGENTS.md` → "Architecture rules".

pub mod ai;
pub mod attempts;
pub mod backup;
pub mod bank;
pub mod commands;
pub mod db;
pub mod error;
pub mod learning;
pub mod metrics;
pub mod migrate;

use tauri::{App, Builder, Manager, Runtime};

/// Registers every command on a Tauri builder.
///
/// Shared by [`run`] and by the IPC tests, which use Tauri's mock runtime.
pub fn register<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder.invoke_handler(tauri::generate_handler![commands::app_info])
}

/// Opens the database in the OS app data directory and makes it available to commands.
fn open_database<R: Runtime>(app: &mut App<R>) -> Result<(), Box<dyn std::error::Error>> {
    let path = db::database_path(&app.path().app_data_dir()?);
    let database = db::Database::open(&path).inspect_err(|error| {
        eprintln!("Gabaritei: {error} ({path:?}: {error:?})");
    })?;
    app.manage(database);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    register(tauri::Builder::default())
        .setup(open_database)
        .run(tauri::generate_context!())
        .expect("error while running the Gabaritei application");
}
