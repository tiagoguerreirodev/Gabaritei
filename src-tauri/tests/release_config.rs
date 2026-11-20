//! Release metadata must agree: one version for the app, the installers and the crate.

use serde_json::Value;
use std::path::Path;

fn read_json(relative: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap()
}

#[test]
fn package_json_and_cargo_toml_share_a_version() {
    let package = read_json("../package.json");
    assert_eq!(
        package["version"].as_str().unwrap(),
        env!("CARGO_PKG_VERSION"),
        "bump package.json and Cargo.toml together"
    );
}

#[test]
fn bundle_version_comes_from_package_json() {
    let config = read_json("tauri.conf.json");
    assert_eq!(config["version"], "../package.json");
}

#[test]
fn bundle_targets_cover_both_platforms_and_the_icons_exist() {
    let config = read_json("tauri.conf.json");
    let targets: Vec<&str> = config["bundle"]["targets"]
        .as_array()
        .expect("explicit bundle targets")
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    for wanted in ["nsis", "app", "dmg"] {
        assert!(targets.contains(&wanted), "missing bundle target {wanted}");
    }
    for icon in config["bundle"]["icon"].as_array().unwrap() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(icon.as_str().unwrap());
        assert!(path.exists(), "missing icon {}", path.display());
    }
}
