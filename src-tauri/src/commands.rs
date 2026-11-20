//! Tauri commands: thin wrappers that call domain functions.
//!
//! Every command must also be added to [`crate::register`] and wrapped in `src/api.ts`.

use serde::Serialize;
use ts_rs::TS;

use crate::error::AppResult;

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder, mock_context, noop_assets};
    use tauri::webview::InvokeRequest;

    /// Sends `cmd` through Tauri's IPC layer on a mock runtime and returns the JSON result.
    fn invoke(cmd: &str) -> Result<Value, Value> {
        let app = crate::register(mock_builder())
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
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
                body: InvokeBody::default(),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .map(|body| body.deserialize::<Value>().unwrap())
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
