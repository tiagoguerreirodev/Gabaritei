use super::*;

/// Longest computer tag kept in a name.
pub(super) const MAX_TAG: usize = 24;

const TAG_HASH_DIGITS: usize = 4;
const TAG_SUFFIX_LENGTH: usize = 1 + TAG_HASH_DIGITS;
const TAG_HASH_MASK: u64 = (1 << (4 * TAG_HASH_DIGITS)) - 1;

/// This computer's current backup tag and the tag used by earlier versions.
#[derive(Debug, Clone, Default)]
pub struct Computer {
    tag: Option<String>,
    previous_tag: Option<String>,
}

impl Computer {
    /// Uses the stable macOS local name when available, otherwise the network host name.
    pub fn from_host_names(local_host_name: Option<&str>, host_name: &str) -> Self {
        Self {
            tag: tag_from(local_host_name, host_name),
            previous_tag: previous_tag_from(host_name),
        }
    }

    pub(super) fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    /// A previous tag only identifies this computer on backups from before the cutoff.
    pub(super) fn owns(&self, backup: &BackupName) -> bool {
        match backup.tag.as_deref() {
            Some(tag) => {
                self.tag.as_deref() == Some(tag)
                    || (backup.stamp.as_str() < PREVIOUS_TAG_UNTIL
                        && self.previous_tag.as_deref() == Some(tag))
            }
            None => self.tag.is_none(),
        }
    }

    /// Only tagged backups are attributed to a different computer.
    pub(super) fn is_other(&self, backup: &BackupName) -> bool {
        backup.tag.is_some() && !self.owns(backup)
    }

    pub(super) fn has_owned(&self, backups: &[BackupName]) -> bool {
        backups.iter().any(|backup| self.owns(backup))
    }
}

/// The tag from the host names the OS gives: the macOS `LocalHostName` when there is a usable
/// one, otherwise the network host name.
fn tag_from(local_host_name: Option<&str>, host_name: &str) -> Option<String> {
    local_host_name.and_then(slug).or_else(|| slug(host_name))
}

/// UTC stamp (as in backup names) from which the previous tag is no longer trusted. #126 (the
/// current derivation) was merged at 2026-11-21 15:57 UTC; the cutoff is a day later because a
/// computer keeps making old-style backups until it runs a new build.
const PREVIOUS_TAG_UNTIL: &str = "20261122-160000-000";

/// The previous derivation used the network host name's first label, ASCII only.
fn previous_tag_from(host_name: &str) -> Option<String> {
    ascii_slug(host_name.split('.').next().unwrap_or_default())
}

/// The first tag derivations: ASCII letters and digits lowercased, other runs one hyphen, cut
/// at [`MAX_TAG`] with no hash.
fn ascii_slug(text: &str) -> Option<String> {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.truncate(MAX_TAG);
    let out = out.trim_end_matches('-');
    (!out.is_empty()).then(|| out.to_owned())
}

/// A plain ASCII letter for a Portuguese or other Western-European accented one.
fn unaccented(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        other => other,
    }
}

/// `Desktop_ABC 12` → `desktop-abc-12`: ASCII letters and digits (accents dropped to their
/// plain letter), other runs become one hyphen. Only the part before the first dot is used: a
/// host name often gets a network domain (`mac.local`, `mac.lan`) that changes with the
/// network, and the tag must not. A name longer than [`MAX_TAG`] is cut and ends with a short
/// hash of the whole name, so two long names that start alike keep different tags.
fn slug(text: &str) -> Option<String> {
    let name = text.split('.').next().unwrap_or_default();
    let mut out = String::new();
    // Combining marks (an accent written apart from its letter) are dropped, not turned into a
    // hyphen: `a` + U+0301 is `a`.
    let letters = name
        .chars()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .flat_map(char::to_lowercase)
        .map(unaccented);
    for c in letters {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    if out.len() <= MAX_TAG {
        return (!out.is_empty()).then(|| out.to_owned());
    }
    // FNV-1a: stable across runs and builds, unlike the standard hasher.
    let hash = out.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    let kept = out[..MAX_TAG - TAG_SUFFIX_LENGTH].trim_end_matches('-');
    Some(format!(
        "{kept}-{:0TAG_HASH_DIGITS$x}",
        hash & TAG_HASH_MASK
    ))
}

#[cfg(test)]
mod tests;
