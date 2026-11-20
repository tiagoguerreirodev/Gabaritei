//! Tauri commands: thin wrappers that call domain functions.
//!
//! Every command must also be added to [`crate::register`] and wrapped in `src/api.ts`.

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_dialog::DialogExt;
use ts_rs::TS;

use crate::bank::catalog::{self, CollectionDetail, CollectionPage, CollectionQuery, TopicTree};
use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::migrate::{self, LegacyImportOutcome, LegacyImportStatus};
use crate::paths::AppPaths;

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

/// Area › matter › topic with question counts.
#[tauri::command]
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
pub fn collection(db: State<'_, Database>, id: String) -> AppResult<CollectionDetail> {
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
        migrate::import_legacy_bank(&db, &paths.images_dir(), &source, &crate::clock::now())
            .map(Some)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
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
        let app = crate::register(mock_builder())
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        app.manage(db);
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock webview builds");
        let url = if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        };
        get_ipc_response(
            &webview,
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

    #[test]
    fn browsing_commands_decode_camel_case_arguments() {
        let db = Database::open_in_memory().unwrap();
        db.with(|c| {
            c.execute_batch(
                "INSERT INTO collections (id, title, kind, contest, created_at) VALUES
                   ('c1', 'Direito › Crase', 'imported', 'ALEPA', '2026-11-19T00:00:00.000Z'),
                   ('c2', 'Informática › SQL', 'imported', 'Perito', '2026-11-19T00:00:00.000Z');",
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

    #[test]
    fn app_info_reports_name_and_crate_version() {
        let info = current_app_info();
        assert_eq!(info.name, "Gabaritei");
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
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
    fn unknown_command_is_rejected() {
        let error = invoke("does_not_exist").expect_err("unknown command fails");
        assert_eq!(error, json!("Command does_not_exist not found"));
    }
}
