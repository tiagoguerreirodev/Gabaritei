//! Tauri commands: thin wrappers that call domain functions.
//!
//! Every command must also be added to [`crate::register`] and wrapped in `src/api.ts`.

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_dialog::DialogExt;
use ts_rs::TS;

use crate::attempts::{self, AttemptView, SessionStart};
use crate::bank::catalog::{self, CollectionDetail, CollectionPage, CollectionQuery, TopicTree};
use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::metrics::{self, HomeSummary, Performance, PerformanceQuery};
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
    db.transaction(|tx| attempts::start(tx, &request, &crate::clock::now()))
}

/// An attempt with every question as the frontend may see it (no unrevealed keys).
#[tauri::command]
pub fn attempt(db: State<'_, Database>, id: String) -> AppResult<AttemptView> {
    db.with(|conn| attempts::load(conn, &id))
}

/// Chooses an option (`None` clears the choice).
#[tauri::command]
pub fn answer(
    db: State<'_, Database>,
    attempt_id: String,
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
    attempt_id: String,
    position: u32,
    option_id: String,
) -> AppResult<AttemptView> {
    db.transaction(|tx| attempts::eliminate(tx, &attempt_id, position, &option_id))
}

/// Sets or clears a question's flag.
#[tauri::command]
pub fn flag(
    db: State<'_, Database>,
    attempt_id: String,
    position: u32,
    flagged: bool,
) -> AppResult<AttemptView> {
    db.transaction(|tx| attempts::flag(tx, &attempt_id, position, flagged))
}

/// Finishes an attempt if `revision` is still current.
#[tauri::command]
pub fn finish(
    db: State<'_, Database>,
    attempt_id: String,
    revision: u32,
) -> AppResult<AttemptView> {
    db.transaction(|tx| attempts::finish(tx, &attempt_id, revision, &crate::clock::now()))
}

/// Annuls a revealed question: it leaves every score until restored.
#[tauri::command]
pub fn annul(db: State<'_, Database>, attempt_id: String, position: u32) -> AppResult<AttemptView> {
    db.transaction(|tx| {
        attempts::set_annulled(tx, &attempt_id, position, true, &crate::clock::now())
    })
}

/// Restores an annulled question to every score.
#[tauri::command]
pub fn unannul(
    db: State<'_, Database>,
    attempt_id: String,
    position: u32,
) -> AppResult<AttemptView> {
    db.transaction(|tx| {
        attempts::set_annulled(tx, &attempt_id, position, false, &crate::clock::now())
    })
}

/// Deletes an unfinished attempt.
#[tauri::command]
pub fn abandon(db: State<'_, Database>, attempt_id: String) -> AppResult<()> {
    db.transaction(|tx| attempts::abandon(tx, &attempt_id))
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
        _app: tauri::App<tauri::test::MockRuntime>,
        webview: tauri::WebviewWindow<tauri::test::MockRuntime>,
    }

    impl MockApp {
        fn new(db: Database) -> Self {
            let app = crate::register(mock_builder())
                .build(mock_context(noop_assets()))
                .expect("mock app builds");
            app.manage(db);
            let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
                .build()
                .expect("mock webview builds");
            Self { _app: app, webview }
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
            tx.execute_batch(&format!(
                "INSERT INTO collections (id, title, kind, created_at)
                   VALUES ('c1', 'Caderno', 'imported', '2026-11-20T00:00:00.000Z');
                 INSERT INTO collection_items VALUES ('c1', 0, '{}'), ('c1', 1, '{}');",
                ids["q1"], ids["q2"]
            ))?;
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

    /// The question's answer, explanation and references are absent from the JSON.
    fn assert_hidden(question: &Value) {
        assert_eq!(question["reveal"], Value::Null, "{question}");
        let text = question.to_string();
        for secret in ["SEGREDO", "REF-", "answerId", "outcome"] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
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
