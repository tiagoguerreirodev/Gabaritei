//! Daily automatic backups and restore (spec §3.6); the export/import data file (#24) builds on
//! the same snapshots.
//!
//! A backup is a folder `backup-<UTC time>-<computer tag>` (see [`computer_tag`]; the tag is
//! missing in names made before tags existed, or when the host name gives none) in the backup
//! folder, holding a consistent copy of the database, the images and a manifest of sha256
//! hashes (see [`snapshot`]). It is written to a hidden temporary folder and renamed, so a crash never leaves a half-written backup
//! that looks complete. The newest 7 are kept. Before a restore replaces anything, a
//! `safety-<UTC time>-<tag>` backup of the current data is made; the newest 5 of those are
//! kept. Each computer rotates only the backups with its own tag.

pub mod datafile;
pub mod relocate;
pub mod snapshot;

use crate::sync::lock;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use ts_rs::TS;

use crate::db::{Database, SettingKey, get_setting, set_setting};
use crate::error::{AppError, AppResult};
use crate::paths::AppPaths;
mod computer;
#[cfg(test)]
mod fixtures;
mod folder;
mod left_behind;
mod name;
mod probe;
mod restore;
mod rotation;
mod service;

pub use computer::*;
pub use folder::*;
use left_behind::*;
use name::*;
use probe::*;
pub use restore::*;
use rotation::*;
pub use service::*;

/// Settings that describe this computer. Restoring or importing data from another computer
/// must not overwrite them.
pub const LOCAL_SETTINGS: &[SettingKey] = &[
    SettingKey::BackupFolder,
    SettingKey::BackupLeftBehind,
    SettingKey::CliPaths,
    // Keep the keychain token paired with this computer's server URL.
    SettingKey::RemNote,
    SettingKey::AiRoles,
    SettingKey::Appearance,
    SettingKey::KeyboardShortcuts,
];

/// Why a backup was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BackupKind {
    /// The daily automatic backup.
    Backup,
    /// "Fazer backup agora".
    Manual,
    /// Made right before a restore or import replaced the data.
    Safety,
}

impl BackupKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Backup => "backup",
            Self::Manual => "manual",
            Self::Safety => "safety",
        }
    }

    fn keep(self) -> usize {
        match self {
            Self::Backup => KEEP_BACKUPS,
            Self::Manual => KEEP_MANUAL,
            Self::Safety => KEEP_SAFETY,
        }
    }
}

/// A backup in the backup folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BackupInfo {
    /// Folder name; identifies the backup in `backup_restore`.
    pub name: String,
    pub kind: BackupKind,
    /// The computer tag in the name (see [`computer_tag`]); none for backups made before tags.
    pub computer: Option<String>,
    /// Whether the backup was made by another computer (a tagged one with another tag). Such
    /// backups can be restored but are never deleted by this computer's rotation.
    pub other_computer: bool,
    pub created_at: String,
    #[ts(type = "number")]
    pub size_bytes: u64,
}

/// The *Dados* section: where backups go, the ones that exist and the last automatic failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BackupStatus {
    pub folder: String,
    pub is_default_folder: bool,
    /// pt-BR reason the last automatic or manual backup failed, until one succeeds.
    pub last_error: Option<String>,
    /// Set while the old folder still holds this computer's backups after a folder change. It is
    /// remembered across restarts until the backups are copied or moved (a copy or move with
    /// failures keeps it), the user dismisses it, the folder is changed again, or the old folder
    /// holds none of this computer's backups.
    pub left_behind: Option<LeftBehind>,
    pub listing: BackupListing,
}

/// Whether the Dados page can show the backup list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
#[ts(export)]
pub enum BackupListing {
    /// Backups, newest first.
    Ready { backups: Vec<BackupInfo> },
    /// The folder answered, but its manifests are still taking too long to read.
    Slow,
    /// The folder could not be opened or did not answer in time.
    FolderUnavailable,
}

/// Backups that stay in the old folder after the backup folder was changed. They are not
/// moved, so they no longer show in the list, and safety backups among them can no longer be
/// used to undo an import or restore from here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LeftBehind {
    pub folder: String,
    pub backups: u32,
    /// How many of them are safety backups ("antes de restaurar").
    pub safety: u32,
    /// The old folder can't be read now (a drive that is unplugged, a share that is offline, a
    /// folder that was deleted). The offer is kept, since the drive may come back, but there is
    /// nothing to count or move: the user can only dismiss it.
    pub unavailable: bool,
}

/// What copying or moving the backups of the old folder did, and the status afterwards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RelocateOutcome {
    pub report: relocate::RelocateReport,
    /// The new folder's status. `left_behind` is what a *move* could not take out of the old
    /// folder; a copy leaves the originals on purpose and reports nothing.
    pub status: BackupStatus,
}

/// A folder the user can open from Configurações → Dados. The frontend only picks one of
/// these; it never sends a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum OpenableFolder {
    /// The app data folder (database, images).
    Data,
    /// The backup folder.
    Backups,
}
