//! Copies of a draft's attachments, kept in the app data folder for the life of the draft.
//!
//! A draft stores attachment *file names* only (never paths). The files themselves are copied
//! here so a retry of the blind check can give the generator the same files it wrote from. They
//! are removed when the draft is imported or discarded.

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// The draft's folder, or `None` if the id is not a single plain name. Draft ids reach commands
/// from the frontend, so `..`, separators and absolute paths must never leave `root`.
fn folder(root: &Path, draft_id: &str) -> Option<PathBuf> {
    let valid = !draft_id.is_empty()
        && draft_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid.then(|| root.join(draft_id))
}

/// Copies `files` into the draft's folder and returns the copies, in order. Names are numbered
/// so two files with the same name don't overwrite each other.
pub fn keep(root: &Path, draft_id: &str, files: &[PathBuf]) -> AppResult<Vec<PathBuf>> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let target = folder(root, draft_id).ok_or_else(bad_id)?;
    std::fs::create_dir_all(&target)?;
    files
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let name = source.file_name().map_or_else(
                || "arquivo".to_owned(),
                |n| n.to_string_lossy().into_owned(),
            );
            let copy = target.join(format!("{:02}-{name}", index + 1));
            std::fs::copy(source, &copy)?;
            Ok(copy)
        })
        .collect()
}

/// The kept copies of a draft, in the order they were attached. Empty if there are none.
pub fn load(root: &Path, draft_id: &str) -> AppResult<Vec<PathBuf>> {
    let Some(target) = folder(root, draft_id) else {
        return Ok(Vec::new());
    };
    if !target.is_dir() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&target)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<_, _>>()
        .map_err(AppError::from)?;
    files.retain(|path| path.is_file());
    files.sort();
    Ok(files)
}

/// Deletes a draft's kept copies. Best effort: a leftover folder is harmless.
pub fn remove(root: &Path, draft_id: &str) {
    if let Some(target) = folder(root, draft_id) {
        let _ = std::fs::remove_dir_all(target);
    }
}

fn bad_id() -> AppError {
    AppError::Internal("invalid draft id".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_are_kept_in_order_and_removed_with_the_draft() {
        let source = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let a = source.path().join("aula.md");
        let b = source.path().join("outra");
        std::fs::create_dir(&b).unwrap();
        let b = b.join("aula.md");
        std::fs::write(&a, "um").unwrap();
        std::fs::write(&b, "dois").unwrap();

        let kept = keep(root.path(), "d1", &[a, b]).unwrap();
        assert_eq!(kept.len(), 2, "same file name must not collide");
        assert_eq!(std::fs::read_to_string(&kept[0]).unwrap(), "um");
        assert_eq!(std::fs::read_to_string(&kept[1]).unwrap(), "dois");
        assert_eq!(load(root.path(), "d1").unwrap(), kept);
        assert!(load(root.path(), "outro").unwrap().is_empty());

        remove(root.path(), "d1");
        assert!(load(root.path(), "d1").unwrap().is_empty());
        remove(root.path(), "d1"); // already gone: no panic
    }

    #[test]
    fn an_id_that_is_not_a_plain_name_never_leaves_the_folder() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("draft-files");
        std::fs::create_dir(&root).unwrap();
        let outside = parent.path().join("keep");
        std::fs::create_dir(&outside).unwrap();
        for id in ["..", "../keep", "", "/etc", "a/b", "a\\b", "."] {
            remove(&root, id);
            assert!(load(&root, id).unwrap().is_empty());
            assert!(keep(&root, id, &[outside.join("x")]).is_err(), "{id:?}");
        }
        assert!(outside.is_dir(), "nothing outside the root was removed");
        assert!(root.is_dir());
    }

    #[test]
    fn a_draft_without_attachments_creates_nothing() {
        let root = tempfile::tempdir().unwrap();
        assert!(keep(root.path(), "d1", &[]).unwrap().is_empty());
        assert!(!root.path().join("d1").exists());
    }
}
