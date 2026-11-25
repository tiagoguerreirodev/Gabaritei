use super::*;

/// Longest computer tag kept in a name.
pub(super) const MAX_TAG: usize = 24;

/// This computer's tag, put at the end of the names of the backups it makes
/// (`backup-20261119-210000-123-<tag>`), so two computers sharing one synced backup folder keep
/// apart: each rotates only its own backups. Derived from the host name when the app runs (on
/// macOS the stable `LocalHostName`), reduced to lowercase letters, digits and single hyphens.
/// `None` when the host name has nothing usable; those backups are untagged, like the ones made before tags existed.
pub fn computer_tag() -> Option<String> {
    #[cfg(test)]
    {
        super::fixtures::TAG
            .with(|tag| tag.borrow().clone())
            .flatten()
    }
    #[cfg(not(test))]
    {
        static TAG: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        TAG.get_or_init(|| {
            tag_from(
                local_host_name().as_deref(),
                &gethostname::gethostname().to_string_lossy(),
            )
        })
        .clone()
    }
}

/// The tag from the host names the OS gives: the macOS `LocalHostName` when there is a usable
/// one, otherwise the network host name.
pub(super) fn tag_from(local_host_name: Option<&str>, host_name: &str) -> Option<String> {
    local_host_name.and_then(slug).or_else(|| slug(host_name))
}

/// UTC stamp (as in backup names) from which the previous tag is no longer trusted. #126 (the
/// current derivation) was merged at 2026-11-21 15:57 UTC; the cutoff is a day later because a
/// computer keeps making old-style backups until it runs a new build.
pub(super) const PREVIOUS_TAG_UNTIL: &str = "20261122-160000-000";

/// The tag this computer's backups carried before the derivation changed (#126): builds from
/// `main` since the tags appeared slugged the network host name (`gethostname()`) up to the
/// first dot, with ASCII letters and digits only (`Máquina.local` → `m-quina`), cut at
/// [`MAX_TAG`]. Backups with it, made before [`PREVIOUS_TAG_UNTIL`], still belong to this
/// computer, so they are rotated, counted for the daily check and moved with the rest, instead
/// of looking like another computer's.
///
/// This trusts a tag rebuilt from today's network host name, which can differ from the one used
/// back then (`air-3.lan` one day, `Air.local` the next): such backups stay unrecognized, and
/// the cutoff keeps any later backup with that tag (another computer's) from being claimed.
pub(super) fn previous_tag() -> Option<String> {
    #[cfg(test)]
    {
        super::fixtures::PREVIOUS.with(|tag| tag.borrow().clone())
    }
    #[cfg(not(test))]
    {
        static TAG: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        TAG.get_or_init(|| previous_tag_from(&gethostname::gethostname().to_string_lossy()))
            .clone()
    }
}

#[cfg_attr(test, allow(dead_code))] // tests set the previous tag directly
pub(super) fn previous_tag_from(host_name: &str) -> Option<String> {
    ascii_slug(host_name.split('.').next().unwrap_or_default())
}

/// The first tag derivations: ASCII letters and digits lowercased, other runs one hyphen, cut
/// at [`MAX_TAG`] with no hash.
pub(super) fn ascii_slug(text: &str) -> Option<String> {
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

/// Whether a backup tagged `tag` (`None`: untagged) made at `stamp` is this computer's by name:
/// it carries this computer's tag, or the [`previous_tag`] from before [`PREVIOUS_TAG_UNTIL`],
/// or it is untagged on a computer with no tag.
pub(super) fn owns_tag(tag: Option<&str>, stamp: &str) -> bool {
    match tag {
        Some(tag) => {
            computer_tag().as_deref() == Some(tag)
                || (stamp < PREVIOUS_TAG_UNTIL && previous_tag().as_deref() == Some(tag))
        }
        None => computer_tag().is_none(),
    }
}

/// macOS keeps a stable name apart from `gethostname()`, which follows the network (a router
/// may hand out `air-3.lan` one day and `Air.local` the next). Other systems have none.
#[cfg(target_os = "macos")]
#[cfg_attr(test, allow(dead_code))] // tests set the tag directly
pub(super) fn local_host_name() -> Option<String> {
    let output = std::process::Command::new("/usr/sbin/scutil")
        .args(["--get", "LocalHostName"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(not(target_os = "macos"))]
#[cfg_attr(test, allow(dead_code))]
pub(super) fn local_host_name() -> Option<String> {
    None
}

/// A plain ASCII letter for a Portuguese or other Western-European accented one.
pub(super) fn unaccented(c: char) -> char {
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
pub(super) fn slug(text: &str) -> Option<String> {
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
    let kept = out[..MAX_TAG - 5].trim_end_matches('-');
    Some(format!("{kept}-{:04x}", hash & 0xffff))
}

/// Whether a backup tagged `tag` was made by another computer than the one running.
pub(super) fn is_other_computer(tag: Option<&str>, stamp: &str) -> bool {
    tag.is_some() && !owns_tag(tag, stamp)
}

/// Whether a backup in a shared folder is not this computer's to copy or delete: another
/// computer's, or (on a computer that has a tag) an untagged one, which can't be told apart.
/// Rotation leaves these alone for the same reason.
pub(super) fn leave_alone(backup: &ParsedName) -> bool {
    !owns_tag(backup.tag.as_deref(), &backup.stamp)
}

#[cfg(test)]
mod tests;
