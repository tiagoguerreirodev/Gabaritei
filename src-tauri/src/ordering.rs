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

pub(crate) fn distinct(values: impl Iterator<Item = Option<String>>) -> Vec<String> {
    let mut values: Vec<String> = values.flatten().collect();
    values.sort_by(|a, b| cmp_pt(a, b));
    values.dedup();
    values
}
