//! Question identity, ported byte for byte from the legacy app (`electron/store.ts`).
//!
//! Migrated questions keep their legacy IDs, so new questions must hash the same way for
//! deduplication to work across both. The legacy code was JavaScript, hence the JS-compatible
//! `trim`, sort order and `JSON.stringify` below.

use std::cmp::Ordering;

use sha2::{Digest, Sha256};

use super::question::QuestionType;
use crate::ids::QuestionId;

/// Lowercase hex SHA-256.
pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// Whitespace removed by ECMAScript `String.prototype.trim` (WhiteSpace + LineTerminator).
/// Differs from Rust's `str::trim`: U+FEFF is trimmed, U+0085 is not.
fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'..='\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{FEFF}'
    )
}

/// `text.trim()` as JavaScript does it.
pub fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

/// JavaScript's default `Array.prototype.sort` order: by UTF-16 code units.
fn cmp_utf16(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// Canonical option ID: the first 24 hex characters of SHA-256 of the option text, untrimmed.
pub fn option_id(text: &str) -> String {
    sha256_hex(text.as_bytes())[..24].to_owned()
}

/// Content fingerprint, used as the question ID:
/// `sha256(JSON.stringify([type, stem.trim(), optionTexts.map(trim).sort(), image || ""]))`.
pub fn fingerprint<'a>(
    kind: QuestionType,
    stem: &str,
    option_texts: impl IntoIterator<Item = &'a str>,
    image: Option<&str>,
) -> QuestionId {
    let mut texts: Vec<&str> = option_texts.into_iter().map(js_trim).collect();
    texts.sort_by(|a, b| cmp_utf16(a, b));
    let payload = serde_json::json!([kind.as_str(), js_trim(stem), texts, image.unwrap_or("")]);
    let json = serde_json::to_string(&payload).expect("strings always serialize");
    sha256_hex(json.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values computed by running the legacy JavaScript functions in Node.
    const TRICKY_STEM: &str = "\u{a0}\u{feff}  Qual \"opção\" usa \\ barra?\n\t\u{85}";
    const TRICKY_OPTIONS: [&str; 5] = [
        "～ til largo",
        "😀 emoji",
        "Aspas \"duplas\" e \\ barra",
        "Controle \u{1} e\nquebra",
        "  ção acentuada\u{3000}",
    ];

    #[test]
    fn trim_matches_javascript() {
        assert_eq!(
            js_trim(TRICKY_STEM),
            "Qual \"opção\" usa \\ barra?\n\t\u{85}"
        );
        assert_eq!(js_trim("\u{feff}x\u{3000}"), "x");
        assert_eq!(js_trim("\u{85}x"), "\u{85}x");
    }

    #[test]
    fn sort_matches_javascript_utf16_order() {
        let mut texts: Vec<&str> = TRICKY_OPTIONS.iter().map(|t| js_trim(t)).collect();
        texts.sort_by(|a, b| cmp_utf16(a, b));
        assert_eq!(
            texts,
            [
                "Aspas \"duplas\" e \\ barra",
                "Controle \u{1} e\nquebra",
                "ção acentuada",
                "😀 emoji",
                "～ til largo",
            ]
        );
    }

    #[test]
    fn option_ids_match_the_legacy_app() {
        let ids: Vec<String> = TRICKY_OPTIONS.iter().map(|t| option_id(t)).collect();
        assert_eq!(
            ids,
            [
                "dd6e950c442288a28cfb6ed3",
                "c5db20e64d86b693d8e312b3",
                "ca39323843e3f8b47aa03e36",
                "fd8abe9aa05c79c389a78dbe",
                "9bf9aa7d8696c7eaeaead390",
            ]
        );
        assert_eq!(option_id("Certo"), "c05ab42722f97eccfea8dbf8");
        assert_eq!(option_id("Errado"), "4ca7c199f5be46e0fdb08d22");
    }

    #[test]
    fn fingerprint_matches_the_legacy_app() {
        assert_eq!(
            fingerprint(
                QuestionType::Multiple,
                TRICKY_STEM,
                TRICKY_OPTIONS,
                Some("0123456789abcdef0123456789abcdef.png"),
            )
            .as_str(),
            "17d77978b89d84a07fa461c1649cef28643f9c4c889a50524e84851598d15799"
        );
        assert_eq!(
            fingerprint(
                QuestionType::Boolean,
                "O STF é o órgão de cúpula do Judiciário.",
                ["Certo", "Errado"],
                None,
            )
            .as_str(),
            "40aa8aba41819ac52b678f4e9168e31e94bf304dd643c4c678c4dc964845cc98"
        );
    }

    #[test]
    fn fingerprint_ignores_option_order() {
        assert_eq!(
            fingerprint(QuestionType::Boolean, "Pergunta", ["Certo", "Errado"], None),
            fingerprint(QuestionType::Boolean, "Pergunta", ["Errado", "Certo"], None)
        );
    }

    #[test]
    fn fingerprint_ignores_outer_whitespace() {
        assert_eq!(
            fingerprint(QuestionType::Boolean, "Pergunta", ["Certo", "Errado"], None),
            fingerprint(
                QuestionType::Boolean,
                "  Pergunta\n",
                [" Certo ", " Errado"],
                None
            )
        );
    }

    #[test]
    fn fingerprint_includes_question_type() {
        assert_ne!(
            fingerprint(QuestionType::Boolean, "Pergunta", ["Certo", "Errado"], None),
            fingerprint(
                QuestionType::Multiple,
                "Pergunta",
                ["Certo", "Errado"],
                None
            )
        );
    }
}
