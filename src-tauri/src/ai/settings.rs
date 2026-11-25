//! AI settings: CLI paths, role configuration and RemNote (spec §3.7).
//!
//! Stored as JSON in the `settings` table. The RemNote token is the exception: it lives in the
//! OS keychain only (see [`super::secrets`]), so it can't reach the database, logs or exports.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::agent::{Agent, Effort};
use super::discovery;
use super::roles::{Role, RoleConfig, RoleSettings};
use super::runner::{JobSpec, RemNoteAccess, is_valid_tool_name, is_write_like};
use super::secrets::Secrets;
use crate::db::{SettingKey, get_setting, set_setting};
use crate::error::{AppError, AppResult};

const MAX_MODEL: usize = 200;
const MAX_PATH: usize = 1_000;
const MAX_URL: usize = 2_000;

/// Optional overrides for where the CLIs are. `None` means "detect automatically".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CliPaths {
    pub claude: Option<String>,
    pub codex: Option<String>,
}

impl CliPaths {
    pub fn get(&self, agent: Agent) -> Option<&str> {
        match agent {
            Agent::Claude => self.claude.as_deref(),
            Agent::Codex => self.codex.as_deref(),
        }
    }
}

/// How RemNote reaches the agents.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RemNoteMode {
    #[default]
    Off,
    /// URL and token set here (token in the keychain).
    Server,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RemNoteSettings {
    pub mode: RemNoteMode,
    pub url: String,
    /// Exact names of the read-only RemNote tools agents may call (an allowlist). Chosen from
    /// what "Testar" finds on the server. Empty means agents get no RemNote tools.
    #[serde(default)]
    pub tools: Vec<String>,
}

/// What Configurações edits (no secrets).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AiSettingsInput {
    pub cli_paths: CliPaths,
    pub roles: RoleSettings,
    pub remnote: RemNoteSettings,
}

/// What Configurações shows: the settings plus whether a token is saved (never the token).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AiSettings {
    pub cli_paths: CliPaths,
    pub roles: RoleSettings,
    pub remnote: RemNoteSettings,
    pub has_remnote_token: bool,
}

/// Just the CLI path overrides (no defaults written, no keychain access).
pub fn load_cli_paths(conn: &Connection) -> AppResult<CliPaths> {
    Ok(get_setting(conn, SettingKey::CliPaths)?.unwrap_or_default())
}

/// Just the role configuration (the saved one, or the defaults), with no writes and no keychain.
pub fn load_roles(conn: &Connection) -> AppResult<RoleSettings> {
    Ok(get_setting(conn, SettingKey::AiRoles)?.unwrap_or_default())
}

/// Loads the settings, writing the defaults the first time (spec §7: set on first run).
pub fn load(conn: &Connection, secrets: &Secrets) -> AppResult<AiSettings> {
    let roles = match get_setting::<RoleSettings>(conn, SettingKey::AiRoles)? {
        Some(roles) => roles,
        None => {
            let defaults = RoleSettings::default();
            set_setting(conn, SettingKey::AiRoles, &defaults)?;
            defaults
        }
    };
    Ok(AiSettings {
        cli_paths: get_setting(conn, SettingKey::CliPaths)?.unwrap_or_default(),
        roles,
        remnote: load_remnote(conn)?,
        // Best effort: a locked or missing keychain must not break every AI feature.
        has_remnote_token: secrets.remnote_token().ok().flatten().is_some(),
    })
}

fn load_remnote(conn: &Connection) -> AppResult<RemNoteSettings> {
    let Some(saved) = get_setting::<serde_json::Value>(conn, SettingKey::RemNote)? else {
        return Ok(RemNoteSettings::default());
    };
    // The retired mode never used the saved URL or tools. Require explicit setup before using them.
    if saved["mode"] == "codexConfig" {
        return Ok(RemNoteSettings::default());
    }
    Ok(serde_json::from_value(saved)?)
}

fn invalid(message: &str) -> AppError {
    AppError::Invalid(message.to_owned())
}

fn clean_path(value: &Option<String>, what: &str) -> AppResult<Option<String>> {
    let Some(text) = value.as_deref().map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    if text.chars().count() > MAX_PATH {
        return Err(AppError::Invalid(format!(
            "O caminho do {what} é longo demais."
        )));
    }
    Ok(Some(text.to_owned()))
}

/// Trims and checks a role's model name. A model starting with `-`, or holding whitespace or
/// control characters, could be read as another CLI option, so it is refused.
pub fn validate_role_config(config: &mut RoleConfig, label: &str) -> AppResult<()> {
    config.model = config.model.trim().to_owned();
    if config.model.is_empty() {
        return Err(AppError::Invalid(format!("Informe o modelo de “{label}”.")));
    }
    if config.model.chars().count() > MAX_MODEL {
        return Err(AppError::Invalid(format!(
            "O nome do modelo de “{label}” é longo demais."
        )));
    }
    if config.model.starts_with('-')
        || config
            .model
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(AppError::Invalid(format!(
            "O nome do modelo de “{label}” é inválido."
        )));
    }
    if config.agent == Agent::Claude && config.effort == Effort::Ultra {
        return Err(AppError::Invalid(format!(
            "O esforço Ultra só existe no Codex. Escolha outro esforço para “{label}”."
        )));
    }
    Ok(())
}

/// Checks and saves the settings. Nothing is written if anything is invalid.
pub fn save(conn: &Connection, input: AiSettingsInput) -> AppResult<()> {
    let mut roles = input.roles;
    for role in Role::ALL {
        let config = match role {
            Role::Generate => &mut roles.generate,
            Role::BlindCheck => &mut roles.blind_check,
            Role::Recheck => &mut roles.recheck,
            Role::Explain => &mut roles.explain,
        };
        validate_role_config(config, role.label())?;
    }
    let cli_paths = CliPaths {
        claude: clean_path(&input.cli_paths.claude, "Claude Code")?,
        codex: clean_path(&input.cli_paths.codex, "Codex")?,
    };
    let url = input.remnote.url.trim().to_owned();
    if input.remnote.mode == RemNoteMode::Server
        && (!(url.starts_with("https://") || url.starts_with("http://"))
            || url.chars().count() > MAX_URL
            || url.contains(char::is_whitespace))
    {
        return Err(invalid(
            "Informe o endereço do RemNote começando com https:// (ou http://).",
        ));
    }
    let mut tools: Vec<String> = Vec::new();
    for tool in &input.remnote.tools {
        let tool = tool.trim();
        if !is_valid_tool_name(tool) {
            return Err(invalid("Há um nome de ferramenta do RemNote inválido."));
        }
        if is_write_like(tool) {
            return Err(AppError::Invalid(format!(
                "A ferramenta “{tool}” do RemNote parece alterar dados, e só ferramentas de leitura são permitidas."
            )));
        }
        if !tools.iter().any(|t| t == tool) {
            tools.push(tool.to_owned());
        }
    }
    let remnote = RemNoteSettings {
        mode: input.remnote.mode,
        url,
        tools,
    };
    set_setting(conn, SettingKey::AiRoles, &roles)?;
    set_setting(conn, SettingKey::CliPaths, &cli_paths)?;
    set_setting(conn, SettingKey::RemNote, &remnote)?;
    Ok(())
}

/// The executable for `agent`: the user's override, else detected.
pub fn resolve_cli(settings: &AiSettings, agent: Agent) -> AppResult<PathBuf> {
    let custom = settings.cli_paths.get(agent).map(Path::new);
    discovery::find_cli(agent, custom).ok_or_else(|| {
        AppError::Invalid(format!(
            "O {} não foi encontrado. Confira Configurações › Agentes.",
            agent.label()
        ))
    })
}

/// RemNote access for a run, or an error if it's switched on but incomplete.
fn remnote_access(settings: &AiSettings, secrets: &Secrets) -> AppResult<Option<RemNoteAccess>> {
    match settings.remnote.mode {
        RemNoteMode::Off => Ok(None),
        RemNoteMode::Server => {
            let token = secrets.remnote_token()?.ok_or_else(|| {
                invalid("O RemNote está ligado, mas falta o token. Informe-o em Configurações › RemNote.")
            })?;
            if settings.remnote.tools.is_empty() {
                return Err(invalid(
                    "Aprove ao menos uma ferramenta de leitura do RemNote em Configurações (use “Testar conexão”).",
                ));
            }
            Ok(Some(RemNoteAccess {
                url: settings.remnote.url.clone(),
                token,
                tools: settings.remnote.tools.clone(),
            }))
        }
    }
}

/// Sources a job may use, chosen per run (spec §3.2).
#[derive(Debug, Clone, Copy, Default)]
pub struct Sources {
    pub remnote: bool,
    pub web: bool,
}

/// Builds the runner spec for `config` (a role's settings, or the user's override of them).
pub fn job_spec(
    settings: &AiSettings,
    secrets: &Secrets,
    config: &RoleConfig,
    schema: serde_json::Value,
    sources: Sources,
) -> AppResult<JobSpec> {
    let cli = resolve_cli(settings, config.agent)?;
    let mut spec = JobSpec::new(
        config.agent,
        cli,
        config.model.clone(),
        config.effort,
        schema,
    );
    spec.fast = config.fast;
    spec.web = sources.web;
    if sources.remnote {
        spec.remnote = remnote_access(settings, secrets)?;
    }
    Ok(spec)
}

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
                    "protocolVersion": "2025-03-26",
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
