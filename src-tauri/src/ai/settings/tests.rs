#![cfg_attr(not(unix), allow(dead_code, unused_imports))] // fake CLIs need `sh`

use super::*;
use crate::ai::agent::Effort;
use crate::db::Database;

fn input(settings: &AiSettings) -> AiSettingsInput {
    AiSettingsInput {
        cli_paths: settings.cli_paths.clone(),
        roles: settings.roles.clone(),
        remnote: settings.remnote.clone(),
    }
}

#[test]
fn first_load_writes_the_defaults() {
    let db = Database::open_in_memory().unwrap();
    let secrets = Secrets::memory();
    let settings = db.with(|conn| load(conn, &secrets)).unwrap();
    assert_eq!(settings.roles, RoleSettings::default());
    assert!(!settings.has_remnote_token);
    // Stored, so a later change to the built-in defaults doesn't silently alter the user's setup.
    let stored: Option<RoleSettings> = db
        .with(|conn| get_setting(conn, SettingKey::AiRoles))
        .unwrap();
    assert_eq!(stored, Some(RoleSettings::default()));
}

#[test]
fn settings_persist() {
    let db = Database::open_in_memory().unwrap();
    let secrets = Secrets::memory();
    let mut edited = db.with(|conn| load(conn, &secrets)).unwrap();
    edited.roles.recheck.agent = Agent::Claude;
    edited.roles.recheck.model = "  claude-opus-5-5 ".into();
    edited.roles.recheck.effort = Effort::High;
    edited.cli_paths.codex = Some(" /opt/codex ".into());
    edited.remnote = RemNoteSettings {
        mode: RemNoteMode::Server,
        url: "https://remnote.example/mcp".into(),
        tools: Vec::new(),
    };
    db.with(|conn| save(conn, input(&edited))).unwrap();

    let reloaded = db.with(|conn| load(conn, &secrets)).unwrap();
    assert_eq!(reloaded.roles.recheck.agent, Agent::Claude);
    assert_eq!(reloaded.roles.recheck.model, "claude-opus-5-5");
    assert_eq!(reloaded.roles.recheck.effort, Effort::High);
    assert_eq!(reloaded.cli_paths.codex.as_deref(), Some("/opt/codex"));
    assert_eq!(reloaded.remnote.mode, RemNoteMode::Server);
    // Other roles are unchanged.
    assert_eq!(reloaded.roles.generate, RoleConfig::default_config());
}

#[test]
fn invalid_settings_change_nothing() {
    let db = Database::open_in_memory().unwrap();
    let secrets = Secrets::memory();
    let original = db.with(|conn| load(conn, &secrets)).unwrap();

    let mut blank_model = original.clone();
    blank_model.roles.explain.model = "   ".into();
    let error = db.with(|conn| save(conn, input(&blank_model))).unwrap_err();
    assert!(error.to_string().contains("Explicação sob demanda"));

    let mut bad_url = original.clone();
    bad_url.remnote = RemNoteSettings {
        mode: RemNoteMode::Server,
        url: "remnote.example".into(),
        tools: Vec::new(),
    };
    assert!(db.with(|conn| save(conn, input(&bad_url))).is_err());

    assert_eq!(db.with(|conn| load(conn, &secrets)).unwrap(), original);
}

#[test]
fn the_token_never_reaches_the_database() {
    let db = Database::open_in_memory().unwrap();
    let secrets = Secrets::memory();
    secrets
        .set_remnote_token(Some("TOKEN-MUITO-SECRETO"))
        .unwrap();
    let mut edited = db.with(|conn| load(conn, &secrets)).unwrap();
    assert!(edited.has_remnote_token);
    edited.remnote = RemNoteSettings {
        mode: RemNoteMode::Server,
        url: "https://remnote.example/mcp".into(),
        tools: Vec::new(),
    };
    db.with(|conn| save(conn, input(&edited))).unwrap();

    // Dump every text value of every table.
    let dump = db
        .with(|conn| {
            let tables: Vec<String> = conn
                .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?
                .query_map([], |row| row.get(0))?
                .collect::<Result<_, _>>()?;
            let mut dump = String::new();
            for table in tables {
                let mut statement = conn.prepare(&format!("SELECT * FROM \"{table}\""))?;
                let columns = statement.column_count();
                let mut rows = statement.query([])?;
                while let Some(row) = rows.next()? {
                    for column in 0..columns {
                        if let Ok(text) = row.get::<_, String>(column) {
                            dump.push_str(&text);
                        }
                    }
                }
            }
            Ok(dump)
        })
        .unwrap();
    assert!(
        dump.contains("remnote.example"),
        "the dump covers the settings"
    );
    assert!(!dump.contains("TOKEN-MUITO-SECRETO"));
    // The view for the frontend says only that a token exists.
    let view = serde_json::to_string(&db.with(|conn| load(conn, &secrets)).unwrap()).unwrap();
    assert!(!view.contains("TOKEN-MUITO-SECRETO"));
    assert!(view.contains("\"hasRemnoteToken\":true"));
}

#[test]
fn blank_token_removes_it() {
    let secrets = Secrets::memory();
    secrets.set_remnote_token(Some("abc")).unwrap();
    secrets.set_remnote_token(Some("  ")).unwrap();
    assert_eq!(secrets.remnote_token().unwrap(), None);
}

#[test]
fn disabled_remnote_grants_no_tools() {
    let secrets = Secrets::memory();
    let db = Database::open_in_memory().unwrap();
    let settings = db.with(|conn| load(conn, &secrets)).unwrap();
    assert_eq!(remnote_access(&settings, &secrets).unwrap(), None);
}

#[test]
fn remnote_server_requires_a_saved_token() {
    let secrets = Secrets::memory();
    let db = Database::open_in_memory().unwrap();
    let mut settings = db.with(|conn| load(conn, &secrets)).unwrap();
    settings.remnote = RemNoteSettings {
        mode: RemNoteMode::Server,
        url: "https://r.example".into(),
        tools: vec!["search".into()],
    };
    let error = remnote_access(&settings, &secrets).unwrap_err();
    assert!(error.to_string().contains("falta o token"));
    secrets.set_remnote_token(Some("t")).unwrap();
    assert_eq!(
        remnote_access(&settings, &secrets).unwrap(),
        Some(RemNoteAccess {
            url: "https://r.example".into(),
            token: "t".into(),
            tools: vec!["search".into()],
        })
    );
}

#[test]
fn retired_codex_remnote_settings_load_as_disabled() {
    let secrets = Secrets::memory();
    secrets.set_remnote_token(Some("previous-token")).unwrap();
    let db = Database::open_in_memory().unwrap();
    let settings = db
        .with(|conn| {
            set_setting(
                conn,
                SettingKey::RemNote,
                &serde_json::json!({
                    "mode": "codexConfig",
                    "url": "https://previous.example/mcp",
                    "tools": ["search"],
                }),
            )?;
            load(conn, &secrets)
        })
        .unwrap();
    assert_eq!(settings.remnote, RemNoteSettings::default());
    assert_eq!(remnote_access(&settings, &secrets).unwrap(), None);
    assert!(!remnote_test(&settings, &secrets).unwrap().ok);
    assert!(settings.has_remnote_token);
}

#[cfg(unix)]
#[test]
fn role_values_feed_the_runner() {
    let dir = tempfile::tempdir().unwrap();
    let fake = crate::ai::runner::tests::write_script(dir.path(), "codex", "exit 0");
    let secrets = Secrets::memory();
    let mut settings = AiSettings {
        cli_paths: CliPaths {
            claude: None,
            codex: Some(fake.to_string_lossy().into_owned()),
        },
        roles: RoleSettings::default(),
        remnote: RemNoteSettings::default(),
        has_remnote_token: false,
    };
    settings.roles.blind_check.effort = Effort::High;
    settings.roles.blind_check.model = "outro-modelo".into();
    settings.roles.blind_check.fast = true;
    let spec = job_spec(
        &settings,
        &secrets,
        &ResolvedClis::new(None, Some(fake.clone())),
        settings.roles.get(Role::BlindCheck),
        serde_json::json!({}),
        Sources {
            remnote: false,
            web: true,
        },
        Vec::new(),
    )
    .unwrap();
    assert_eq!(spec.agent, Agent::Codex);
    assert_eq!(spec.cli, fake);
    assert_eq!(spec.model, "outro-modelo");
    assert_eq!(spec.effort, Effort::High);
    assert!(spec.fast);
    assert!(spec.web);
}

#[test]
fn a_missing_cli_is_a_clear_error() {
    let error = ResolvedClis::new(None, None)
        .require(Agent::Claude)
        .unwrap_err();
    assert!(error.to_string().contains("Claude Code não foi encontrado"));
}

#[test]
fn approved_tools_are_trimmed_deduplicated_and_validated() {
    let db = Database::open_in_memory().unwrap();
    let secrets = Secrets::memory();
    let mut edited = db.with(|conn| load(conn, &secrets)).unwrap();
    edited.remnote = RemNoteSettings {
        mode: RemNoteMode::Server,
        url: "https://r.example".into(),
        tools: vec![" search ".into(), "search".into(), "read_note".into()],
    };
    db.with(|conn| save(conn, input(&edited))).unwrap();
    let saved = db.with(|conn| load(conn, &secrets)).unwrap();
    assert_eq!(saved.remnote.tools, ["search", "read_note"]);

    for bad in ["delete note", "search,Bash", "Bash(*)", "a*b"] {
        edited.remnote.tools = vec![bad.into()];
        assert!(db.with(|conn| save(conn, input(&edited))).is_err(), "{bad}");
    }
    // A tool that looks like a write is refused even though its name is well formed.
    edited.remnote.tools = vec!["search".into(), "create_note".into()];
    let error = db.with(|conn| save(conn, input(&edited))).unwrap_err();
    assert!(error.to_string().contains("só ferramentas de leitura"));
}

#[test]
fn tool_names_come_from_json_or_event_stream_replies() {
    let json =
        r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"search"},{"name":"create_note"}]}}"#;
    assert_eq!(
        tool_names(&reply_json(json).unwrap()),
        ["create_note", "search"]
    );
    let sse = format!("event: message\ndata: {json}\n\n");
    assert_eq!(
        tool_names(&reply_json(&sse).unwrap()),
        ["create_note", "search"]
    );
    assert_eq!(tool_names(&serde_json::json!({})), Vec::<String>::new());
}

/// A one-connection-per-request HTTP server that answers like an MCP server.
fn fake_mcp_server(expected_token: &'static str) -> String {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming().take(3) {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            let mut authorized = false;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let lower = line.to_lowercase();
                if let Some(value) = lower.strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                if lower.starts_with("authorization:") {
                    authorized = line.trim().ends_with(&format!("Bearer {expected_token}"));
                }
                if line == "\r\n" || line.is_empty() {
                    break;
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let body = String::from_utf8(body).unwrap();
            let (status, extra, reply) = if !authorized {
                ("401 Unauthorized", "", String::new())
            } else if body.contains("\"initialize\"") {
                (
                    "200 OK",
                    "Mcp-Session-Id: s1\r\n",
                    r#"{"jsonrpc":"2.0","id":1,"result":{}}"#.into(),
                )
            } else if body.contains("tools/list") {
                (
                    "200 OK",
                    "",
                    "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[{\"name\":\"search\"},{\"name\":\"create_note\"}]}}\n\n".into(),
                )
            } else {
                ("202 Accepted", "", String::new())
            };
            write!(
                stream,
                "HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            )
            .unwrap();
        }
    });
    url
}

fn server_settings(url: String) -> AiSettings {
    AiSettings {
        cli_paths: CliPaths::default(),
        roles: RoleSettings::default(),
        remnote: RemNoteSettings {
            mode: RemNoteMode::Server,
            url,
            tools: Vec::new(),
        },
        has_remnote_token: true,
    }
}

#[test]
fn the_remnote_test_connects_and_lists_the_servers_tools() {
    let secrets = Secrets::memory();
    secrets.set_remnote_token(Some("bom-token")).unwrap();
    let result = remnote_test(&server_settings(fake_mcp_server("bom-token")), &secrets).unwrap();
    assert!(result.ok, "{}", result.message);
    assert_eq!(result.tools, ["create_note", "search"]);
}

#[test]
fn the_remnote_test_reports_a_rejected_token() {
    let secrets = Secrets::memory();
    secrets.set_remnote_token(Some("token-errado")).unwrap();
    let result = remnote_test(&server_settings(fake_mcp_server("bom-token")), &secrets).unwrap();
    assert!(!result.ok);
    assert!(result.message.contains("recusou o token"));
    assert!(result.tools.is_empty());
}

#[test]
fn the_remnote_test_needs_a_token_and_a_reachable_server() {
    let secrets = Secrets::memory();
    let settings = server_settings("http://127.0.0.1:9/mcp".into());
    assert!(
        remnote_test(&settings, &secrets)
            .unwrap()
            .message
            .contains("token")
    );
    secrets.set_remnote_token(Some("t")).unwrap();
    assert!(
        remnote_test(&settings, &secrets)
            .unwrap()
            .message
            .contains("conectar")
    );
}

#[test]
fn ultra_effort_is_refused_for_claude_and_kept_for_codex() {
    let mut claude = RoleConfig {
        agent: Agent::Claude,
        model: "sonnet".to_owned(),
        effort: Effort::Ultra,
        fast: false,
    };
    let error = validate_role_config(&mut claude, "Gerar questões").unwrap_err();
    assert!(error.to_string().contains("Ultra"), "{error}");

    let mut codex = RoleConfig {
        agent: Agent::Codex,
        model: "gpt-6.2-sol".to_owned(),
        effort: Effort::Ultra,
        fast: false,
    };
    assert!(validate_role_config(&mut codex, "Gerar questões").is_ok());
}

#[test]
fn load_roles_reads_only_the_roles_and_writes_nothing() {
    let db = Database::open_in_memory().unwrap();
    let roles = db.with(|conn| load_roles(conn)).unwrap();
    assert_eq!(roles, RoleSettings::default());
    let rows: i64 = db
        .with(|conn| Ok(conn.query_row("SELECT COUNT(*) FROM settings", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(rows, 0, "no defaults are written");
}
