//! Resolve the binary that Codex will re-execute inside its sandbox.

use std::path::{Path, PathBuf};

pub(super) struct Installation {
    pub(super) executable: PathBuf,
    pub(super) readable: PathBuf,
}

pub(super) fn resolve(cli: &Path) -> Option<Installation> {
    let cli = cli.canonicalize().ok()?;
    let executable = match cli.extension().and_then(|extension| extension.to_str()) {
        Some("js") if cli.ends_with("@openai/codex/bin/codex.js") => {
            npm_binary(cli.parent()?.parent()?)?
        }
        Some("cmd" | "ps1") => npm_binary(&cli.parent()?.join("node_modules/@openai/codex"))?,
        _ => cli,
    };
    let executable = executable.canonicalize().ok()?;
    if !executable.is_file() {
        return None;
    }

    // Packaged builds include companion executables and resources next to their `bin` folder.
    // A loose binary (including a Homebrew cask symlink) only needs the resolved executable.
    let readable = executable
        .parent()
        .filter(|bin| bin.ends_with("bin"))
        .and_then(Path::parent)
        .filter(|root| root.join("codex-package.json").is_file())
        .unwrap_or(&executable)
        .to_owned();
    is_safe_root(&readable).then_some(Installation {
        executable,
        readable,
    })
}

fn npm_binary(package: &Path) -> Option<PathBuf> {
    let (package_name, target, binary) = npm_target()?;
    let scope = package.parent()?;
    [
        package.join("node_modules/@openai").join(package_name),
        scope.join(package_name),
        package.to_owned(),
    ]
    .into_iter()
    .map(|dependency| dependency.join("vendor").join(target))
    .flat_map(|directory| {
        [
            directory.join("bin").join(binary),
            directory.join("codex").join(binary),
        ]
    })
    .find(|candidate| candidate.is_file())
}

fn npm_target() -> Option<(&'static str, &'static str, &'static str)> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => ("codex-linux-x64", "x86_64-unknown-linux-musl", "codex"),
        ("linux", "aarch64") => ("codex-linux-arm64", "aarch64-unknown-linux-musl", "codex"),
        ("macos", "x86_64") => ("codex-darwin-x64", "x86_64-apple-darwin", "codex"),
        ("macos", "aarch64") => ("codex-darwin-arm64", "aarch64-apple-darwin", "codex"),
        ("windows", "x86_64") => ("codex-win32-x64", "x86_64-pc-windows-msvc", "codex.exe"),
        ("windows", "aarch64") => ("codex-win32-arm64", "aarch64-pc-windows-msvc", "codex.exe"),
        _ => return None,
    })
}

fn is_safe_root(root: &Path) -> bool {
    let homes: Vec<_> = ["HOME", "USERPROFILE"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .map(|home| home.canonicalize().unwrap_or(home))
        .collect();
    let codex_homes = homes
        .iter()
        .map(|home| home.join(".codex"))
        .chain(std::env::var_os("CODEX_HOME").map(PathBuf::from));
    root.parent().is_some()
        && homes.iter().all(|home| !home.starts_with(root))
        && codex_homes
            .map(|home| home.canonicalize().unwrap_or(home))
            .all(|home| safe_relative_to_codex_home(root, &home))
}

fn safe_relative_to_codex_home(root: &Path, codex_home: &Path) -> bool {
    !codex_home.starts_with(root)
        && (!root.starts_with(codex_home) || root.starts_with(codex_home.join("packages")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "synthetic").unwrap();
    }

    #[test]
    fn npm_uses_the_platform_package_containing_the_native_binary() {
        let Some((name, target, binary)) = npm_target() else {
            return;
        };
        let temp = tempfile::tempdir().unwrap();
        let scope = temp.path().join("node_modules/@openai");
        let package = scope.join("codex");
        let wrapper = package.join("bin/codex.js");
        let runtime = scope.join(name).join("vendor").join(target);
        let executable = runtime.join("bin").join(binary);
        file(&wrapper);
        file(&executable);
        file(&runtime.join("codex-package.json"));

        let installation = resolve(&wrapper).unwrap();
        assert_eq!(installation.executable, executable.canonicalize().unwrap());
        assert_eq!(installation.readable, runtime.canonicalize().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_loose_binary_grants_only_the_resolved_file() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("Caskroom/codex/0.172.0/codex-native");
        let link = temp.path().join("codex");
        file(&executable);
        std::os::unix::fs::symlink(&executable, &link).unwrap();

        let installation = resolve(&link).unwrap();
        let executable = executable.canonicalize().unwrap();
        assert_eq!(installation.executable, executable);
        assert_eq!(installation.readable, executable);
    }

    #[test]
    fn codex_home_grants_are_limited_to_the_packages_subtree() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path().join(".codex");
        for root in [
            temp.path().to_owned(),
            codex_home.clone(),
            codex_home.join("other"),
        ] {
            assert!(!safe_relative_to_codex_home(&root, &codex_home));
        }
        assert!(safe_relative_to_codex_home(
            &codex_home.join("packages/standalone/releases/version"),
            &codex_home,
        ));
    }
}
