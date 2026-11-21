//! Finding the CLIs and asking whether they are installed and logged in.
//!
//! Apps launched from the macOS Finder or the Windows Start menu get a short `PATH`, so besides
//! `PATH` we look in the usual install folders. The user can always point to the executable.
//! Credential files are never read: login state comes from the CLI's own status command.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Serialize;
use ts_rs::TS;

use super::agent::Agent;

const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// Folders that commonly hold the CLIs, relative to the home folder.
const HOME_SUBDIRS: &[&str] = &[
    ".local/bin",
    ".claude/local",
    ".npm-global/bin",
    ".bun/bin",
    ".volta/bin",
    ".cargo/bin",
    "AppData/Roaming/npm",
    "AppData/Local/Programs/claude",
    "AppData/Local/Microsoft/WinGet/Links",
];

/// Absolute folders that commonly hold the CLIs.
const SYSTEM_DIRS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"];

fn file_names(agent: Agent) -> Vec<String> {
    let name = agent.binary();
    if cfg!(windows) {
        vec![format!("{name}.exe"), format!("{name}.cmd")]
    } else {
        vec![name.to_owned()]
    }
}

/// Every place the executable could be, in priority order: the override, then `PATH`, then the
/// usual install folders. Pure, so it can be tested without touching the disk.
pub fn candidates(
    agent: Agent,
    custom: Option<&Path>,
    path_var: Option<OsString>,
    home: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = path_var
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    if let Some(home) = &home {
        folders.extend(HOME_SUBDIRS.iter().map(|sub| home.join(sub)));
    }
    folders.extend(SYSTEM_DIRS.iter().map(PathBuf::from));

    let mut found: Vec<PathBuf> = custom.map(Path::to_path_buf).into_iter().collect();
    for folder in folders {
        for file in file_names(agent) {
            let path = folder.join(file);
            if !found.contains(&path) {
                found.push(path);
            }
        }
    }
    found
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// The first existing executable for `agent`. An override that doesn't exist is an error the
/// caller shows, so it never silently falls back to another install.
pub fn find_cli(agent: Agent, custom: Option<&Path>) -> Option<PathBuf> {
    if let Some(custom) = custom {
        return is_executable(custom).then(|| custom.to_path_buf());
    }
    candidates(agent, None, std::env::var_os("PATH"), home_dir())
        .into_iter()
        .find(|path| is_executable(path))
}

/// What the "Testar" button shows for one CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CliStatus {
    pub agent: Agent,
    /// Where the executable was found.
    pub path: Option<String>,
    pub version: Option<String>,
    /// `None` when it couldn't be determined.
    pub logged_in: Option<bool>,
    /// pt-BR summary, safe to show as is.
    pub message: String,
}

pub(super) struct Captured {
    pub success: bool,
    pub stdout: String,
    #[allow(dead_code)]
    pub stderr: String,
}

/// Starts `command`, retrying briefly when the OS answers "text file busy" (`ETXTBSY`, Unix only).
///
/// That error means another thread forked while the executable was still open for writing, which
/// is only a race (it shows up when a script is written right before it runs) and clears within
/// milliseconds.
pub(super) fn spawn_retrying(command: &mut Command) -> std::io::Result<std::process::Child> {
    let mut attempts = 0;
    loop {
        match command.spawn() {
            Err(error)
                if error.kind() == std::io::ErrorKind::ExecutableFileBusy && attempts < 50 =>
            {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
}

/// Runs a short probe command with a timeout, with no stdin. Keeps up to 1 MiB of output.
pub(super) fn capture(cli: &Path, args: &[&str]) -> Option<Captured> {
    capture_limited(cli, args, 1 << 20)
}

/// Like [`capture`], keeping at most `limit` bytes of each stream. Output past the limit is read
/// and discarded, so a chatty child can't block on a full pipe.
pub(super) fn capture_limited(cli: &Path, args: &[&str], limit: u64) -> Option<Captured> {
    capture_with(cli, args, limit, PROBE_TIMEOUT)
}

/// How long to wait for the output readers once the child has ended or been killed. A killed
/// child's own children may keep the pipes open, and must not hold the probe hostage.
const READER_GRACE: Duration = Duration::from_secs(2);

/// Like [`capture_limited`], with a custom timeout.
pub(super) fn capture_with(
    cli: &Path,
    args: &[&str],
    limit: u64,
    timeout: Duration,
) -> Option<Captured> {
    let mut command = Command::new(cli);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = spawn_retrying(&mut command).ok()?;
    let mut stdout = child.stdout.take()?;
    let mut stderr = child.stderr.take()?;
    let (out_tx, out_rx) = std::sync::mpsc::channel();
    let (err_tx, err_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = out_tx.send(read_all(&mut stdout, limit));
    });
    std::thread::spawn(move || {
        let _ = err_tx.send(read_all(&mut stderr, limit));
    });
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let stdout = out_rx.recv_timeout(READER_GRACE).unwrap_or_default();
    let stderr = err_rx.recv_timeout(READER_GRACE).unwrap_or_default();
    Some(Captured {
        success: status.is_some_and(|s| s.success()),
        stdout,
        stderr,
    })
}

fn read_all(reader: &mut impl std::io::Read, limit: u64) -> String {
    let mut bytes = Vec::new();
    let _ = reader.take(limit).read_to_end(&mut bytes);
    let _ = std::io::copy(reader, &mut std::io::sink());
    String::from_utf8_lossy(&bytes).into_owned()
}

/// First non-empty line of the version output, e.g. `2.1.329 (Claude Code)`.
fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

/// Login state from the CLI's own status command. `claude auth status` prints JSON with
/// `loggedIn`; `codex login status` exits 0 only when logged in.
fn login_state(agent: Agent, cli: &Path) -> Option<bool> {
    match agent {
        Agent::Claude => {
            let out = capture(cli, &["auth", "status"])?;
            let value: serde_json::Value = serde_json::from_str(out.stdout.trim()).ok()?;
            value.get("loggedIn")?.as_bool()
        }
        Agent::Codex => capture(cli, &["login", "status"]).map(|out| out.success),
    }
}

/// Finds the CLI (or uses `custom`) and reports its version and login state.
pub fn cli_test(agent: Agent, custom: Option<&Path>) -> CliStatus {
    let Some(cli) = find_cli(agent, custom) else {
        let message = if custom.is_some() {
            format!(
                "O caminho informado para o {} não existe ou não é executável.",
                agent.label()
            )
        } else {
            format!(
                "O {} não foi encontrado. Instale-o ou informe o caminho do executável.",
                agent.label()
            )
        };
        return CliStatus {
            agent,
            path: None,
            version: None,
            logged_in: None,
            message,
        };
    };
    let version = capture(&cli, &["--version"])
        .filter(|out| out.success)
        .and_then(|out| first_line(&out.stdout).or_else(|| first_line(&out.stderr)));
    let logged_in = version.as_ref().and_then(|_| login_state(agent, &cli));
    let message = match (&version, logged_in) {
        (None, _) => format!(
            "O {} foi encontrado, mas não respondeu. Verifique a instalação.",
            agent.label()
        ),
        (Some(_), Some(true)) => format!("{}: pronto para uso.", agent.label()),
        (Some(_), Some(false)) => format!(
            "{}: instalado, mas sem login. Entre pelo terminal ({} login).",
            agent.label(),
            agent.binary()
        ),
        (Some(_), None) => format!(
            "{}: instalado, mas não foi possível confirmar o login.",
            agent.label()
        ),
    };
    CliStatus {
        agent,
        path: Some(cli.to_string_lossy().into_owned()),
        version,
        logged_in,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_override_comes_first_then_path_then_the_usual_folders() {
        let list = candidates(
            Agent::Codex,
            Some(Path::new("/custom/codex")),
            Some(std::env::join_paths(["/a", "/b"]).unwrap()),
            Some(PathBuf::from("/home/u")),
        );
        assert_eq!(list[0], Path::new("/custom/codex"));
        // Every file name of a folder is tried (`.exe` before `.cmd` on Windows) before the
        // next folder.
        let names = file_names(Agent::Codex);
        let first = names.len();
        for (i, name) in names.iter().enumerate() {
            assert_eq!(list[1 + i], Path::new("/a").join(name));
            assert_eq!(list[1 + first + i], Path::new("/b").join(name));
        }
        let name = &names[0];
        assert!(list.contains(&Path::new("/home/u/.local/bin").join(name)));
        assert!(list.contains(&Path::new("/opt/homebrew/bin").join(name)));
    }

    #[test]
    fn a_short_path_still_finds_the_homebrew_install() {
        // A Finder-launched app has PATH=/usr/bin:/bin; Homebrew lives elsewhere.
        let list = candidates(
            Agent::Claude,
            None,
            Some(OsString::from("/usr/bin:/bin")),
            None,
        );
        assert!(list.iter().any(|p| p.starts_with("/opt/homebrew/bin")));
    }

    #[test]
    fn no_candidate_repeats() {
        let list = candidates(
            Agent::Claude,
            None,
            Some(OsString::from("/usr/local/bin")),
            None,
        );
        let mut unique = list.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), list.len());
    }

    #[test]
    fn a_missing_override_is_reported_instead_of_falling_back() {
        assert_eq!(
            find_cli(Agent::Claude, Some(Path::new("/no/such/claude"))),
            None
        );
        let status = cli_test(Agent::Claude, Some(Path::new("/no/such/claude")));
        assert_eq!(status.path, None);
        assert!(status.message.contains("caminho informado"));
    }

    #[cfg(unix)]
    #[test]
    fn cli_test_reads_version_and_login_from_the_clis_own_commands() {
        let dir = tempfile::tempdir().unwrap();
        let fake = crate::ai::runner::tests::write_script(
            dir.path(),
            "claude",
            r#"case "$1" in
  --version) echo "9.9.9 (Claude Code)";;
  auth) echo '{"loggedIn": false}';;
esac"#,
        );
        let status = cli_test(Agent::Claude, Some(&fake));
        assert_eq!(status.version.as_deref(), Some("9.9.9 (Claude Code)"));
        assert_eq!(status.logged_in, Some(false));
        assert!(status.message.contains("sem login"));
    }

    #[cfg(unix)]
    #[test]
    fn codex_login_state_follows_the_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let logged_in = crate::ai::runner::tests::write_script(
            dir.path(),
            "codex",
            r#"case "$1" in --version) echo "codex-cli 1.0.0";; login) exit 0;; esac"#,
        );
        assert_eq!(
            cli_test(Agent::Codex, Some(&logged_in)).logged_in,
            Some(true)
        );
        let logged_out = crate::ai::runner::tests::write_script(
            dir.path(),
            "codex2",
            r#"case "$1" in --version) echo "codex-cli 1.0.0";; login) exit 1;; esac"#,
        );
        assert_eq!(
            cli_test(Agent::Codex, Some(&logged_out)).logged_in,
            Some(false)
        );
    }

    #[cfg(unix)]
    #[test]
    #[cfg(unix)]
    fn capture_returns_after_the_timeout_even_if_a_grandchild_keeps_the_pipes_open() {
        let dir = tempfile::tempdir().unwrap();
        // The background `sleep` outlives the killed shell and keeps stdout and stderr open.
        let script =
            crate::ai::runner::tests::write_script(dir.path(), "slow", "sleep 6 &\nsleep 6");
        let started = std::time::Instant::now();
        let out = capture_with(&script, &[], 1 << 20, Duration::from_millis(300))
            .expect("the probe still reports");
        assert!(!out.success);
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "waited {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn probes_survive_scripts_written_while_other_threads_fork() {
        // Regression for #60: writing a script and running it at once can hit ETXTBSY.
        let dir = tempfile::tempdir().unwrap();
        std::thread::scope(|scope| {
            for thread in 0..8 {
                let dir = dir.path();
                scope.spawn(move || {
                    for round in 0..60 {
                        let script = crate::ai::runner::tests::write_script(
                            dir,
                            &format!("codex-{thread}-{round}"),
                            "exit 0",
                        );
                        let out = capture(&script, &["login", "status"]);
                        assert!(out.is_some_and(|out| out.success));
                    }
                });
            }
        });
    }
}
