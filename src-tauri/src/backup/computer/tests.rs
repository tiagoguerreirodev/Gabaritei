use super::*;
use crate::backup::fixtures::*;

#[test]
fn the_tag_keeps_accented_letters_as_plain_ones() {
    assert_eq!(slug("Máquina"), Some("maquina".into()));
    assert_eq!(slug("Ação-PC Été"), Some("acao-pc-ete".into()));
    assert_eq!(slug("Çãõ"), Some("cao".into()));
    // Letters with no plain form still separate words instead of vanishing.
    assert_eq!(slug("日本-pc"), Some("pc".into()));
}

#[test]
fn accents_written_as_separate_marks_are_dropped_too() {
    // "Máquina" as `a` + U+0301, the way macOS and some keyboards write it.
    assert_eq!(slug("Ma\u{301}quina"), Some("maquina".into()));
    assert_eq!(slug("A\u{327}a\u{303}o"), Some("aao".into()));
}

#[test]
fn long_host_names_that_start_alike_get_different_tags() {
    let a = slug("estacao-de-trabalho-do-escritorio-a").unwrap();
    let b = slug("estacao-de-trabalho-do-escritorio-b").unwrap();
    assert_ne!(a, b);
    assert!(a.len() <= MAX_TAG && b.len() <= MAX_TAG);
    assert!(parse(&format!("backup-20261119-210000-123-{a}")).is_some());
    // The same name always gives the same tag, and a short one is left as it is.
    assert_eq!(slug("estacao-de-trabalho-do-escritorio-a").unwrap(), a);
    assert_eq!(slug("desktop-abc"), Some("desktop-abc".into()));
    assert_eq!(slug(&"x".repeat(MAX_TAG)), Some("x".repeat(MAX_TAG)));
}

#[test]
fn the_macos_local_host_name_wins_over_the_network_host_name() {
    // `gethostname()` follows the network on a Mac ("air-3.lan", "Air.local"); the
    // LocalHostName does not.
    assert_eq!(
        tag_from(Some("MacBook-Air"), "air-3.lan"),
        Some("macbook-air".into())
    );
    assert_eq!(tag_from(None, "Desktop.local"), Some("desktop".into()));
    assert_eq!(tag_from(Some("  "), "Desktop"), Some("desktop".into()));
    assert_eq!(tag_from(Some("???"), "???"), None);
}

#[test]
fn the_tag_comes_from_the_host_name_as_a_plain_slug() {
    assert_eq!(slug("DESKTOP-ABC12").as_deref(), Some("desktop-abc12"));
    // A network domain is not part of the name: it changes with the network.
    assert_eq!(slug("MacBook-Pro.local").as_deref(), Some("macbook-pro"));
    assert_eq!(slug("macbook-pro.lan").as_deref(), Some("macbook-pro"));
    assert_eq!(
        slug("  Mac_Book Pro (2)").as_deref(),
        Some("mac-book-pro-2")
    );
    assert_eq!(slug("Máquina").as_deref(), Some("maquina"));
    assert_eq!(slug("a".repeat(40).as_str()).unwrap().len(), MAX_TAG);
    assert_eq!(slug("---"), None);
    assert_eq!(slug(""), None);
    // Too long, with a hyphen where it is cut: still a valid tag, never two hyphens in a row.
    let cut = slug(&format!("{}-b", "a".repeat(MAX_TAG - 6))).unwrap();
    assert!(cut.len() <= MAX_TAG && !cut.contains("--") && !cut.ends_with('-'));
    // Every slug is a name the parser accepts.
    let tag = slug("Meu PC #1").unwrap();
    assert!(parse(&format!("backup-20261119-210000-123-{tag}")).is_some());
}

#[test]
fn the_previous_tag_is_the_first_label_of_the_host_name_with_ascii_only() {
    assert_eq!(previous_tag_from("mac").as_deref(), Some("mac"));
    assert_eq!(previous_tag_from("Mac.local").as_deref(), Some("mac"));
    // Accents were dropped to hyphens back then.
    assert_eq!(previous_tag_from("Máquina.lan").as_deref(), Some("m-quina"));
    // Cut at the limit, without the hash of the current derivation.
    assert_eq!(
        previous_tag_from(&"a".repeat(40)),
        Some("a".repeat(MAX_TAG))
    );
    assert_eq!(previous_tag_from("???"), None);
}

#[test]
fn the_old_style_tag_is_only_trusted_before_the_new_derivation_shipped() {
    let f = fixture();
    let folder = default_folder(&f.paths);
    as_computer("m-quina");
    backup(&f, "2026-11-22T15:00:00.000Z");
    // Another computer whose tag happens to equal this one's old tag, made later.
    backup(&f, "2026-11-22T17:00:00.000Z");
    as_computer("maquina");
    PREVIOUS.with(|tag| *tag.borrow_mut() = Some("m-quina".to_owned()));

    let all = list(&folder);
    let at = |stamp: &str| all.iter().find(|b| b.name.contains(stamp)).unwrap();
    assert!(!at("20261122-150000").other_computer);
    assert!(
        at("20261122-170000").other_computer,
        "made after the cutoff"
    );
}
