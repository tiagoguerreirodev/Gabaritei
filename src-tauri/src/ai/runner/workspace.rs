//! The temporary run directory and the separate private Claude configuration directory.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::{Agent, JobSpec, RemNoteAccess, RunError};
use crate::ai::claude;

/// The run's temporary folder and the files the CLI is pointed at. Deleted on drop.
pub(in crate::ai) struct Workdir {
    dir: tempfile::TempDir,
    /// Holds `mcp.json`, which contains the RemNote token. It is a separate folder, not under
    /// the agent's working directory, so the agent's file tools can't read the token.
    _config_dir: Option<tempfile::TempDir>,
    pub schema: PathBuf,
    /// Claude's role instructions, passed by file. On Windows an npm install is a `.cmd` shim,
    /// and Rust refuses newlines in the arguments of batch files (and `cmd.exe` caps a command
    /// line at 8191 characters), so long multi-line text must not travel as an argument.
    pub instructions: Option<PathBuf>,
    /// Where Codex writes its last message (`-o`).
    pub output: PathBuf,
    /// Claude's MCP config, if RemNote is on (outside the working directory).
    pub mcp_config: Option<PathBuf>,
    pub has_attachments: bool,
}

impl Workdir {
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub(super) fn prepare(spec: &JobSpec) -> Result<Self, RunError> {
        let workspace = |error: std::io::Error| RunError::Workspace(error.to_string());
        let dir = tempfile::Builder::new()
            .prefix("gabaritei-run-")
            .tempdir()
            .map_err(workspace)?;
        let schema = dir.path().join("schema.json");
        std::fs::write(
            &schema,
            serde_json::to_vec_pretty(&spec.schema).unwrap_or_default(),
        )
        .map_err(workspace)?;

        if !spec.attachments.is_empty() {
            let folder = dir.path().join("anexos");
            std::fs::create_dir(&folder).map_err(workspace)?;
            for (index, source) in spec.attachments.iter().enumerate() {
                let name = source
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "arquivo".into());
                let target = folder.join(format!("{}-{}", index + 1, sanitize(&name)));
                std::fs::copy(source, target).map_err(|error| {
                    RunError::Workspace(format!("{}: {error}", sanitize(&name)))
                })?;
            }
        }

        // Files for the CLI itself live outside the run folder, so the agent's file tools
        // (scoped to it) can't list or cite them. Holds the RemNote token and Claude's
        // role instructions.
        let claude_files = spec.agent == Agent::Claude
            && (!spec.instructions.is_empty() || spec.remnote.is_some());
        let config_dir = if claude_files {
            Some(
                tempfile::Builder::new()
                    .prefix("gabaritei-config-")
                    .tempdir()
                    .map_err(workspace)?,
            )
        } else {
            None
        };
        let mut mcp_config = None;
        let mut instructions = None;
        if let Some(folder) = &config_dir {
            if let (Agent::Claude, Some(RemNoteAccess { url, token, .. })) =
                (&spec.agent, &spec.remnote)
            {
                let path = folder.path().join("mcp.json");
                write_private(&path, &claude::mcp_config(url, token)).map_err(workspace)?;
                mcp_config = Some(path);
            }
            if !spec.instructions.is_empty() {
                let path = folder.path().join("instrucoes.md");
                std::fs::write(&path, &spec.instructions).map_err(workspace)?;
                instructions = Some(path);
            }
        }

        Ok(Self {
            _config_dir: config_dir,
            output: dir.path().join("saida.json"),
            schema,
            instructions,
            mcp_config,
            has_attachments: !spec.attachments.is_empty(),
            dir,
        })
    }
}

/// Writes a file only the current user can read (on Unix; Windows folders under the user's
/// temp directory are already private to them).
fn write_private(path: &Path, content: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(content.as_bytes())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, content)
    }
}

/// A file name that is safe inside the run folder on every platform.
fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    cleaned.trim_start_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_names_are_made_safe() {
        assert_eq!(sanitize("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitize(".bashrc"), "bashrc");
        assert_eq!(sanitize("Lei 8.112 (1990).pdf"), "Lei_8.112__1990_.pdf");
    }
}
