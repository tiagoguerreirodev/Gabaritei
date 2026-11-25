use std::cmp::Ordering;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Lowercase, accent-free text for search and Portuguese alphabetical order.
pub fn fold(text: &str) -> String {
    text.nfd()
        .filter(|c| !is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase()
}

/// Portuguese-friendly order: accents and case only break ties.
pub(crate) fn cmp_pt(a: &str, b: &str) -> Ordering {
    fold(a).cmp(&fold(b)).then_with(|| a.cmp(b))
}

/// Compares present values first. Absent values always sort last.
pub(crate) fn some_first<T>(
    left: Option<&T>,
    right: Option<&T>,
    compare: impl FnOnce(&T, &T) -> Ordering,
) -> Ordering {
    match (left, right) {
        (Some(a), Some(b)) => compare(a, b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}
