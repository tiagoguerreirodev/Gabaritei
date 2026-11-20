//! Where the app keeps its files, all inside the OS app data directory.

use std::path::{Path, PathBuf};

/// Folders and files under the app data directory. Managed as Tauri state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    data_dir: PathBuf,
}

impl AppPaths {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// The SQLite database file.
    pub fn database(&self) -> PathBuf {
        crate::db::database_path(&self.data_dir)
    }

    /// Question images, named by content hash.
    pub fn images_dir(&self) -> PathBuf {
        self.data_dir.join("images")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_lives_under_the_data_dir() {
        let paths = AppPaths::new("/data/app");
        assert_eq!(paths.database(), Path::new("/data/app/gabaritei.sqlite"));
        assert_eq!(paths.images_dir(), Path::new("/data/app/images"));
    }
}
