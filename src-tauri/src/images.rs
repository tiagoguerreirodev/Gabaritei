//! Serves question images to the webview through the `image` URI scheme (spec §3.1).
//!
//! URLs look like `image://localhost/<name>` (Linux, macOS) or `http://image.localhost/<name>`
//! (Windows); the frontend builds them with `convertFileSrc(name, "image")`. Only names that
//! match the image pattern are served, and only from `<app data>/images/`: anything else,
//! including path traversal, gets 404. No other folder is reachable.

use std::path::Path;

use tauri::http::{Method, Request, Response, StatusCode, header};

use crate::bank::question::is_valid_image_name;

/// URI scheme registered for question images.
pub const SCHEME: &str = "image";

fn status(code: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(code)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Vec::new())
        .expect("static response is valid")
}

fn content_type(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, ext)| ext) {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    }
}

/// The response for any request that can't be served.
pub fn not_found() -> Response<Vec<u8>> {
    status(StatusCode::NOT_FOUND)
}

/// Answers one request for an image stored in `images_dir`.
pub fn respond(images_dir: &Path, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    if request.method() != Method::GET {
        return status(StatusCode::METHOD_NOT_ALLOWED);
    }
    // The path is exactly "/<name>". The name pattern allows no "/", "\\", "%" or "..",
    // so encoded or nested paths never reach the filesystem.
    let Some(name) = request.uri().path().strip_prefix('/') else {
        return status(StatusCode::NOT_FOUND);
    };
    if !is_valid_image_name(name) {
        return status(StatusCode::NOT_FOUND);
    }
    match std::fs::read(images_dir.join(name)) {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type(name))
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
            // Names are content hashes, so an image never changes.
            .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
            .body(bytes)
            .expect("image response is valid"),
        Err(_) => status(StatusCode::NOT_FOUND),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const NAME: &str = "0123456789abcdef0123456789abcdef.png";
    const BYTES: &[u8] = b"\x89PNG fake";

    struct Folders {
        _dir: tempfile::TempDir,
        images: std::path::PathBuf,
    }

    /// `<tmp>/app/images/<NAME>` plus a secret file next to `images/` and an image-named file
    /// outside it, to prove neither is reachable.
    fn folders() -> Folders {
        let dir = tempfile::tempdir().unwrap();
        let images = dir.path().join("app").join("images");
        fs::create_dir_all(&images).unwrap();
        fs::write(images.join(NAME), BYTES).unwrap();
        fs::write(dir.path().join("app").join("gabaritei.sqlite"), b"secret").unwrap();
        fs::write(
            dir.path()
                .join("app")
                .join("ffffffffffffffffffffffffffffffff.png"),
            b"outside",
        )
        .unwrap();
        Folders { _dir: dir, images }
    }

    fn get(images: &Path, uri: &str) -> Response<Vec<u8>> {
        respond(images, &Request::get(uri).body(Vec::new()).unwrap())
    }

    #[test]
    fn serves_a_stored_image_with_its_content_type() {
        let f = folders();
        for uri in [
            format!("image://localhost/{NAME}"),
            format!("http://image.localhost/{NAME}"),
        ] {
            let response = get(&f.images, &uri);
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            assert_eq!(response.body(), BYTES);
            assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");
            assert_eq!(
                response.headers()[header::X_CONTENT_TYPE_OPTIONS],
                "nosniff"
            );
        }
    }

    #[test]
    fn unknown_names_are_not_found() {
        let f = folders();
        let missing = "fedcba9876543210fedcba9876543210.png";
        assert_eq!(
            get(&f.images, &format!("image://localhost/{missing}")).status(),
            404
        );
        assert_eq!(get(&f.images, "image://localhost/").status(), 404);
        assert_eq!(get(&f.images, "image://localhost/foto.png").status(), 404);
    }

    #[test]
    fn path_traversal_and_other_folders_are_not_reachable() {
        let f = folders();
        for uri in [
            "image://localhost/../gabaritei.sqlite",
            "image://localhost/..%2Fgabaritei.sqlite",
            "image://localhost/%2e%2e/ffffffffffffffffffffffffffffffff.png",
            "image://localhost/../ffffffffffffffffffffffffffffffff.png",
            "image://localhost/sub/0123456789abcdef0123456789abcdef.png",
            "image://localhost//etc/passwd",
            "image://localhost/..\\gabaritei.sqlite",
            "image://localhost/0123456789abcdef0123456789abcdef.png%00.txt",
        ] {
            let response = get(&f.images, uri);
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
            assert!(response.body().is_empty(), "{uri}");
        }
    }

    #[test]
    fn only_get_is_allowed() {
        let f = folders();
        let request = Request::post(format!("image://localhost/{NAME}"))
            .body(Vec::new())
            .unwrap();
        assert_eq!(
            respond(&f.images, &request).status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
    }

    #[test]
    fn content_types_follow_the_extension() {
        assert_eq!(content_type("a.png"), "image/png");
        assert_eq!(content_type("a.jpg"), "image/jpeg");
        assert_eq!(content_type("a.jpeg"), "image/jpeg");
        assert_eq!(content_type("a.webp"), "image/webp");
    }
}
