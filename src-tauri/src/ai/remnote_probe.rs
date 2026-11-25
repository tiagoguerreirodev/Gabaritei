use serde::Serialize;
use std::time::Duration;
use ts_rs::TS;

use super::secrets::Secrets;
use super::settings::{AiSettings, RemNoteMode};
use crate::error::AppResult;

/// Result of the RemNote "Testar" button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RemNoteTest {
    pub ok: bool,
    /// pt-BR, safe to show.
    pub message: String,
    /// Tool names the server offers (empty unless the server answered). The user picks the
    /// read-only ones to allow.
    pub tools: Vec<String>,
}

const MCP_PROTOCOL_VERSION: &str = "2025-03-26";
const TEST_TIMEOUT: Duration = Duration::from_secs(15);

fn test_result(ok: bool, message: &str, tools: Vec<String>) -> AppResult<RemNoteTest> {
    Ok(RemNoteTest {
        ok,
        message: message.to_owned(),
        tools,
    })
}

/// One JSON-RPC call to an MCP server over HTTP. The reply is JSON or a server-sent event.
struct McpReply {
    status: u16,
    session: Option<String>,
    body: String,
}

fn mcp_post(
    agent: &ureq::Agent,
    url: &str,
    token: &str,
    session: Option<&str>,
    body: &serde_json::Value,
) -> Result<McpReply, ureq::Error> {
    let mut request = agent
        .post(url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream");
    if let Some(session) = session {
        request = request.header("Mcp-Session-Id", session);
    }
    let mut response = request.send(body.to_string().as_bytes())?;
    let status = response.status().as_u16();
    let session = response
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let body = response.body_mut().read_to_string().unwrap_or_default();
    Ok(McpReply {
        status,
        session,
        body,
    })
}

/// The JSON-RPC message in a reply: plain JSON, or the first `data:` line of an event stream.
fn reply_json(body: &str) -> Option<serde_json::Value> {
    serde_json::from_str(body.trim()).ok().or_else(|| {
        body.lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .find_map(|data| serde_json::from_str(data.trim()).ok())
    })
}

/// Tool names from a `tools/list` reply.
fn tool_names(reply: &serde_json::Value) -> Vec<String> {
    let mut names: Vec<String> = reply["result"]["tools"]
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Checks that the RemNote MCP server answers with the saved token, and lists its tools.
pub fn remnote_test(settings: &AiSettings, secrets: &Secrets) -> AppResult<RemNoteTest> {
    match settings.remnote.mode {
        RemNoteMode::Off => test_result(false, "O RemNote está desligado.", Vec::new()),
        RemNoteMode::Server => {
            let Some(token) = secrets.remnote_token()? else {
                return test_result(false, "Informe o token do RemNote primeiro.", Vec::new());
            };
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .timeout_global(Some(TEST_TIMEOUT))
                .http_status_as_error(false)
                .build()
                .into();
            let url = &settings.remnote.url;
            let initialize = serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "gabaritei", "version": env!("CARGO_PKG_VERSION") }
                }
            });
            let Ok(first) = mcp_post(&agent, url, &token, None, &initialize) else {
                return test_result(
                    false,
                    "Não foi possível conectar ao endereço do RemNote.",
                    Vec::new(),
                );
            };
            match first.status {
                200..=299 => {}
                401 | 403 => return test_result(false, "O RemNote recusou o token.", Vec::new()),
                code => {
                    return test_result(
                        false,
                        &format!("O RemNote respondeu com erro (código {code})."),
                        Vec::new(),
                    );
                }
            }
            // Listing the tools is best effort: a server that connects but won't list them
            // still passes the connection test.
            let session = first.session.as_deref();
            let _ = mcp_post(
                &agent,
                url,
                &token,
                session,
                &serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            );
            let tools = mcp_post(
                &agent,
                url,
                &token,
                session,
                &serde_json::json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
            )
            .ok()
            .and_then(|reply| reply_json(&reply.body))
            .map(|json| tool_names(&json))
            .unwrap_or_default();
            test_result(true, "Conexão com o RemNote funcionando.", tools)
        }
    }
}

#[cfg(test)]
mod tests;
