use super::*;
use std::fmt;
use time::UtcOffset;
use time::macros::format_description;

pub(super) const TEMP_PREFIX: &str = ".tmp-";
const STAMP_LENGTH: usize = "YYYYMMDD-HHMMSS-mmm".len();
const DATE_SEPARATOR: usize = "YYYYMMDD".len();
const SECOND_SEPARATOR: usize = "YYYYMMDD-HHMMSS".len();

/// The fixed-width UTC time in a backup name.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Stamp(String);

impl Stamp {
    pub(super) fn at(now: OffsetDateTime) -> Self {
        let format =
            format_description!("[year][month][day]-[hour][minute][second]-[subsecond digits:3]");
        let stamp = now
            .to_offset(UtcOffset::UTC)
            .format(&format)
            .expect("a UTC timestamp always formats");
        Self(stamp)
    }

    fn parse(text: &str) -> Option<Self> {
        let valid = text.len() == STAMP_LENGTH
            && text.bytes().enumerate().all(|(index, byte)| match index {
                DATE_SEPARATOR | SECOND_SEPARATOR => byte == b'-',
                _ => byte.is_ascii_digit(),
            });
        valid.then(|| Self(text.to_owned()))
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Stamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// A backup folder name and the kind, UTC time, and computer tag it encodes.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct BackupName {
    pub(super) name: String,
    pub(super) kind: BackupKind,
    pub(super) stamp: Stamp,
    pub(super) tag: Option<String>,
}

impl BackupName {
    pub(super) fn new(kind: BackupKind, stamp: Stamp, computer: &Computer) -> Self {
        let tag = computer.tag().map(str::to_owned);
        let mut name = format!("{}-{stamp}", kind.prefix());
        if let Some(tag) = &tag {
            name.push('-');
            name.push_str(tag);
        }
        Self {
            name,
            kind,
            stamp,
            tag,
        }
    }

    /// Reads `<kind>-<stamp>` or `<kind>-<stamp>-<tag>`.
    pub(super) fn parse(name: &str) -> Option<Self> {
        let (kind, rest) = [BackupKind::Backup, BackupKind::Manual, BackupKind::Safety]
            .into_iter()
            .find_map(|kind| Some((kind, name.strip_prefix(kind.prefix())?.strip_prefix('-')?)))?;
        let (stamp, suffix) = rest.split_at_checked(STAMP_LENGTH)?;
        let stamp = Stamp::parse(stamp)?;
        let tag = if suffix.is_empty() {
            None
        } else {
            Some(
                suffix
                    .strip_prefix('-')
                    .filter(|tag| valid_tag(tag))?
                    .to_owned(),
            )
        };
        Some(Self {
            name: name.to_owned(),
            kind,
            stamp,
            tag,
        })
    }
}

fn valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && !tag.starts_with('-')
        && !tag.ends_with('-')
        && !tag.contains("--")
        && tag.len() <= MAX_TAG
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// The kind a folder name says it is, if it is a backup name at all.
pub(super) fn parse_name(name: &str) -> Option<BackupKind> {
    BackupName::parse(name).map(|parsed| parsed.kind)
}

#[cfg(test)]
mod tests;
