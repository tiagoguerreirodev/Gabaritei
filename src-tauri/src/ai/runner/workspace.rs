//! The temporary run directory and the separate private Claude configuration directory.

use std::path::{Path, PathBuf};

use super::{Agent, JobSpec, RemNoteAccess, RunError};
use crate::ai::claude;

/// The run's temporary folder and the files the CLI is pointed at. Deleted on drop.
pub(in crate::ai) struct Workdir {
    dir: tempfile::TempDir,
    schema: PathBuf,
    output: PathBuf,
    has_attachments: bool,
    claude: Option<ClaudeConfig>,
}

impl Workdir {
    pub(super) fn prepare(spec: &JobSpec) -> Result<Self, RunError> {
        let dir = tempdir("gabaritei-run-")?;
        let schema = dir.path().join("schema.json");
        std::fs::write(
            &schema,
            serde_json::to_vec_pretty(&spec.schema).unwrap_or_default(),
        )
        .map_err(workspace_error)?;
        copy_attachments(dir.path(), &spec.attachments)?;
        Ok(Self {
            output: dir.path().join("saida.json"),
            has_attachments: !spec.attachments.is_empty(),
            claude: ClaudeConfig::prepare(spec)?,
            schema,
            dir,
        })
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn schema(&self) -> &Path {
        &self.schema
    }

    pub fn output(&self) -> &Path {
        &self.output
    }

    pub fn has_attachments(&self) -> bool {
        self.has_attachments
    }

    pub fn instructions(&self) -> Option<&Path> {
        self.claude.as_ref()?.instructions.as_deref()
    }

    pub fn mcp_config(&self) -> Option<&Path> {
        self.claude.as_ref()?.mcp_config.as_deref()
    }
}

/// Configuration files live outside the run folder, so Claude's file tools cannot read
/// the token or list the role instructions.
struct ClaudeConfig {
    _dir: tempfile::TempDir,
    /// Instructions stay out of arguments so Windows .cmd shims accept multi-line text.
    instructions: Option<PathBuf>,
    mcp_config: Option<PathBuf>,
}

impl ClaudeConfig {
    fn prepare(spec: &JobSpec) -> Result<Option<Self>, RunError> {
        if spec.agent != Agent::Claude || (spec.instructions.is_empty() && spec.remnote.is_none()) {
            return Ok(None);
        }
        let dir = tempdir("gabaritei-config-")?;
        let mcp_config = if let Some(RemNoteAccess { url, token, .. }) = &spec.remnote {
            let path = dir.path().join("mcp.json");
            write_private(&path, &claude::mcp_config(url, token)).map_err(workspace_error)?;
            Some(path)
        } else {
            None
        };
        let instructions = if spec.instructions.is_empty() {
            None
        } else {
            let path = dir.path().join("instrucoes.md");
            std::fs::write(&path, &spec.instructions).map_err(workspace_error)?;
            Some(path)
        };
        Ok(Some(Self {
            _dir: dir,
            mcp_config,
            instructions,
        }))
    }
}

fn tempdir(prefix: &str) -> Result<tempfile::TempDir, RunError> {
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .map_err(workspace_error)
}

fn copy_attachments(dir: &Path, sources: &[PathBuf]) -> Result<(), RunError> {
    if sources.is_empty() {
        return Ok(());
    }
    let folder = dir.join("anexos");
    std::fs::create_dir(&folder).map_err(workspace_error)?;
    for (index, source) in sources.iter().enumerate() {
        let name = source
            .file_name()
            .map(|name| sanitize(&name.to_string_lossy()))
            .unwrap_or_else(|| "arquivo".into());
        let target = folder.join(format!("{}-{name}", index + 1));
        std::fs::copy(source, target)
            .map_err(|error| RunError::Workspace(format!("{name}: {error}")))?;
    }
    Ok(())
}

fn workspace_error(error: std::io::Error) -> RunError {
    RunError::Workspace(error.to_string())
}

/// Writes a file only the current user can read (on Unix; Windows folders under the user's
/// temp directory are already private to them).
fn write_private(path: &Path, content: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
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
