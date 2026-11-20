fn main() {
    // On Windows the unit-test executables link Tauri's dialog code, which needs Common
    // Controls v6. The app gets its manifest from `tauri_build`; test binaries don't, and
    // fail at startup with STATUS_ENTRYPOINT_NOT_FOUND. Embed a manifest for them (issue #49).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-tests=/MANIFESTINPUT:{dir}\\windows-test.manifest");
        println!("cargo:rerun-if-changed=windows-test.manifest");
    }
    tauri_build::build()
}
