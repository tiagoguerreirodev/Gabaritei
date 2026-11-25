use super::*;

/// `YYYYMMDD-HHMMSS-mmm` from a timestamp like `2026-11-19T21:00:00.123Z` (see [`crate::clock`]).
pub(super) fn stamp(created_at: &str) -> String {
    let digits: String = created_at.chars().filter(char::is_ascii_digit).collect();
    format!("{}-{}-{}", &digits[0..8], &digits[8..14], &digits[14..17])
}

/// What a backup folder name says.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct ParsedName {
    /// The folder name this was read from.
    pub(super) name: String,
    pub(super) kind: BackupKind,
    /// `YYYYMMDD-HHMMSS-mmm`, UTC.
    pub(super) stamp: String,
    pub(super) tag: Option<String>,
}

/// Reads a backup folder name (`<kind>-<stamp>` or `<kind>-<stamp>-<tag>`), if it is one.
pub(super) fn parse(name: &str) -> Option<ParsedName> {
    let (kind, rest) = [BackupKind::Backup, BackupKind::Manual, BackupKind::Safety]
        .into_iter()
        .find_map(|kind| Some((kind, name.strip_prefix(kind.prefix())?.strip_prefix('-')?)))?;
    let (stamp, tag) = match rest.split_at_checked(19)? {
        (stamp, "") => (stamp, None),
        (stamp, tag) => (stamp, Some(tag.strip_prefix('-')?)),
    };
    let shape = stamp.bytes().enumerate().all(|(index, byte)| match index {
        8 | 15 => byte == b'-',
        _ => byte.is_ascii_digit(),
    });
    let tag_ok = tag.is_none_or(|tag| {
        !tag.is_empty()
            && !tag.starts_with('-')
            && !tag.ends_with('-')
            && !tag.contains("--")
            && tag.len() <= MAX_TAG
            && tag
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    });
    (shape && tag_ok).then(|| ParsedName {
        name: name.to_owned(),
        kind,
        stamp: stamp.to_owned(),
        tag: tag.map(str::to_owned),
    })
}

/// The kind a folder name says it is, if it is a backup name at all.
pub(super) fn parse_name(name: &str) -> Option<BackupKind> {
    parse(name).map(|parsed| parsed.kind)
}

/// `2026-11-19T10:00:00.000Z` from a backup name's stamp `20261119-100000-000` (UTC), so it
/// compares with [`crate::clock::format`] output.
pub(super) fn stamp_to_iso(stamp: &str) -> String {
    format!(
        "{}-{}-{}T{}:{}:{}.{}Z",
        &stamp[0..4],
        &stamp[4..6],
        &stamp[6..8],
        &stamp[9..11],
        &stamp[11..13],
        &stamp[13..15],
        &stamp[16..19]
    )
}

#[cfg(test)]
mod tests;
