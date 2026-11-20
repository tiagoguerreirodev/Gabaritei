fn main() {
    // On Windows the unit-test executable links Tauri's dialog code, which needs Common
    // Controls v6. The app gets its manifest from `tauri_build`; the test executable doesn't,
    // and fails at startup with STATUS_ENTRYPOINT_NOT_FOUND (issue #49). Cargo has no link
    // argument for unit tests only, and the app executable already carries a manifest, so this
    // is opt-in: set GABARITEI_TEST_MANIFEST=1 when running `cargo test` on Windows.
    println!("cargo:rerun-if-env-changed=GABARITEI_TEST_MANIFEST");
    if std::env::var_os("GABARITEI_TEST_MANIFEST").is_some()
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{dir}\\windows-test.manifest");
        println!("cargo:rerun-if-changed=windows-test.manifest");
    }
    tauri_build::build()
}
