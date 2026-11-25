//! Serves question images to the webview through the `image` URI scheme (spec §3.1).
//!
//! URLs look like `image://localhost/<name>` (Linux, macOS) or `http://image.localhost/<name>`
//! (Windows); the frontend builds them with `convertFileSrc(name, "image")`. Only names that
//! match the image pattern are served, and only from `<app data>/images/`: anything else,
//! including path traversal, gets 404. No other folder is reachable.

use std::path::Path;

use tauri::http::{Method, Request, Response, StatusCode, header};

/// URI scheme registered for question images.
pub const SCHEME: &str = "image";

pub(crate) enum ImageFormat {
    Png,
    Jpeg,
    Webp,
}

impl ImageFormat {
    /// Matches a lowercase hex hash followed by a supported extension.
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        let (stem, extension) = name.split_once('.')?;
        if !(24..=64).contains(&stem.len())
            || !stem.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return None;
        }
        match extension {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "webp" => Some(Self::Webp),
            _ => None,
        }
    }

    fn content_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(code)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Vec::new())
        .expect("static response is valid")
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
    let Some(format) = ImageFormat::from_name(name) else {
        return status(StatusCode::NOT_FOUND);
    };
    match std::fs::read(images_dir.join(name)) {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, format.content_type())
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
    fn requests_cannot_reach_paths_outside_the_image_folder() {
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
        for (extension, content_type) in [
            ("png", "image/png"),
            ("jpg", "image/jpeg"),
            ("jpeg", "image/jpeg"),
            ("webp", "image/webp"),
        ] {
            let name = format!("0123456789abcdef0123456789abcdef.{extension}");
            assert_eq!(
                ImageFormat::from_name(&name).unwrap().content_type(),
                content_type
            );
        }
    }
}
