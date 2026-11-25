use super::*;
use crate::ai::roles::RoleSettings;
use crate::ai::settings::{CliPaths, RemNoteSettings};

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
