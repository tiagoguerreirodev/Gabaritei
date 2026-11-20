//! A small log file in the app data folder.
//!
//! Release builds on Windows use the GUI subsystem and a Finder-launched macOS app has no
//! useful stderr, so `eprintln!` goes nowhere in production. Messages written here are kept in
//! `<app data>/gabaritei.log`, which the user can open from the data folder.
//!
//! What goes in: technical detail of a failure the user only sees as a short pt-BR message
//! (an I/O error, a path, an error code). What never goes in: question or answer text,
//! prompts, agent output, credentials or anything from the CLIs' own files (AGENTS.md rules 2
//! and 3). The file is capped: when it reaches [`MAX_BYTES`] it becomes `gabaritei.log.old`
//! (replacing the previous one), so the two together stay under twice that size.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

/// The log file's name inside the app data folder.
pub const FILE_NAME: &str = "gabaritei.log";

/// The previous log, kept when the current one fills up.
const OLD_FILE_NAME: &str = "gabaritei.log.old";

/// The size at which the log is rotated.
const MAX_BYTES: u64 = 256 * 1024;

/// Longest message kept; the rest is cut so one huge error cannot fill the log.
const MAX_LINE_CHARS: usize = 2000;

static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
static WRITING: Mutex<()> = Mutex::new(());

/// Chooses the folder the log lives in. Only the first call counts; later ones are ignored.
pub fn init(data_dir: &Path) {
    let _ = LOG_DIR.set(data_dir.to_path_buf());
}

/// Records `message` in the log file (when [`init`] ran) and on stderr, for `cargo tauri dev`.
/// Never fails: a log that cannot be written must not break what it describes.
pub fn error(message: impl std::fmt::Display) {
    let message = message.to_string();
    eprintln!("Gabaritei: {message}");
    if let Some(dir) = LOG_DIR.get() {
        let _guard = WRITING.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = append(dir, &crate::clock::now(), &message, MAX_BYTES);
    }
}

/// One line per message: line breaks become spaces and a very long message is cut.
fn one_line(message: &str) -> String {
    let flat: String = message
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    match flat.char_indices().nth(MAX_LINE_CHARS) {
        Some((end, _)) => format!("{}…", &flat[..end]),
        None => flat,
    }
}

fn append(dir: &Path, at: &str, message: &str, max_bytes: u64) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let file = dir.join(FILE_NAME);
    if fs::metadata(&file).is_ok_and(|meta| meta.len() >= max_bytes) {
        fs::rename(&file, dir.join(OLD_FILE_NAME))?;
    }
    let mut out = OpenOptions::new().create(true).append(true).open(file)?;
    writeln!(out, "{at} {}", one_line(message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_one_timestamped_line_per_message() {
        let dir = tempfile::tempdir().unwrap();
        append(dir.path(), "2026-11-19T10:00:00.000Z", "first", MAX_BYTES).unwrap();
        append(
            dir.path(),
            "2026-11-19T10:00:01.000Z",
            "second\nwith a break",
            MAX_BYTES,
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join(FILE_NAME)).unwrap(),
            "2026-11-19T10:00:00.000Z first\n2026-11-19T10:00:01.000Z second with a break\n"
        );
    }

    #[test]
    fn a_full_log_becomes_the_old_one_and_the_old_one_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..6 {
            append(dir.path(), "t", &format!("message {n}"), 20).unwrap();
        }
        let current = fs::read_to_string(dir.path().join(FILE_NAME)).unwrap();
        let old = fs::read_to_string(dir.path().join(OLD_FILE_NAME)).unwrap();
        assert!(current.contains("message 5"));
        assert!(!old.contains("message 0"), "the oldest log is dropped");
        assert!(fs::metadata(dir.path().join(FILE_NAME)).unwrap().len() < 40);
    }

    #[test]
    fn a_huge_message_is_cut_at_a_character_boundary() {
        let line = one_line(&"é".repeat(MAX_LINE_CHARS + 50));
        assert_eq!(line.chars().count(), MAX_LINE_CHARS + 1);
        assert!(line.ends_with('…'));
    }

    #[test]
    fn creates_the_folder_when_it_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("not/yet");
        append(&nested, "t", "hello", MAX_BYTES).unwrap();
        assert!(nested.join(FILE_NAME).is_file());
    }
}
