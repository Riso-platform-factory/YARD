//! The build script's check of the product config: a missing or invalid
//! identifier, icon, or URL scheme fails with the file and the key.

#[path = "../build_support.rs"]
mod build_support;

use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// PNG's CRC-32, written here apart from the build script's.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A chunk: length, kind, data, and the CRC of kind and data.
fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = (data.len() as u32).to_be_bytes().to_vec();
    bytes.extend(kind);
    bytes.extend(data);
    bytes.extend(crc32(&bytes[4..]).to_be_bytes());
    bytes
}

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// The signature and a valid `IHDR` chunk.
fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut data = width.to_be_bytes().to_vec();
    data.extend(height.to_be_bytes());
    data.extend([8, 6, 0, 0, 0]);
    let mut bytes = SIGNATURE.to_vec();
    bytes.extend(chunk(b"IHDR", &data));
    bytes
}

/// A PNG's chunk structure: the header, then `IEND`.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = png_header(width, height);
    bytes.extend(chunk(b"IEND", &[]));
    bytes
}

/// Writes `product.toml` with `keys` and, when given, `icon.png`, in a
/// folder removed when the returned guard drops.
fn config(keys: &str, icon: Option<&[u8]>) -> (TempDir, PathBuf) {
    config_with_scheme(keys, icon, "url_scheme = \"sample\"\n")
}

fn config_with_scheme(keys: &str, icon: Option<&[u8]>, scheme: &str) -> (TempDir, PathBuf) {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("product.toml");
    std::fs::write(
        &path,
        format!("name = \"sample\"\nversion = \"1.0\"\n{keys}{scheme}"),
    )
    .unwrap();
    if let Some(icon) = icon {
        std::fs::write(folder.path().join("icon.png"), icon).unwrap();
    }
    (folder, path)
}

const IDENTIFIER: &str = "identifier = \"com.example.sample\"\n";
const ICON: &str = "identifier = \"com.example.sample\"\nicon = \"icon.png\"\n";

/// The error for the config, which names the file and the key.
fn refused((_folder, path): (TempDir, PathBuf), key: &str) -> String {
    let error = build_support::read_product_config(&path).unwrap_err();
    assert!(
        error.contains(&path.display().to_string()) && error.contains(&format!("`{key}`")),
        "{error}"
    );
    error
}

#[test]
fn a_valid_config_gives_the_identifier_and_the_icon_s_absolute_path() {
    let icon = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("icon.png")).unwrap();
    let (folder, path) = config(ICON, Some(&icon));
    let config = build_support::read_product_config(&path).unwrap();
    assert_eq!(config.name, "sample");
    assert_eq!(config.version, "1.0");
    assert_eq!(config.identifier, "com.example.sample");
    assert!(config.icon.is_absolute());
    assert_eq!(
        config.icon,
        folder.path().join("icon.png").canonicalize().unwrap()
    );
}

#[test]
fn a_square_png_of_exactly_512_is_accepted() {
    let (_folder, path) = config(ICON, Some(&png(512, 512)));
    build_support::read_product_config(&path).unwrap();
}

#[test]
fn the_app_s_own_config_is_valid() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("product.toml");
    let config = build_support::read_product_config(&path).unwrap();
    assert_eq!(config.identifier, "vn.risotech.riso");
    assert_eq!(config.url_scheme, "riso");
    assert!(config.icon.ends_with("icon.png"));
}

#[test]
fn a_url_scheme_accepts_lowercase_ascii_letters_digits_and_punctuation() {
    let icon = png(512, 512);
    for scheme in ["a", "sample2+preview-1.test"] {
        let (_folder, path) =
            config_with_scheme(ICON, Some(&icon), &format!("url_scheme = {scheme:?}\n"));
        let config = build_support::read_product_config(&path).unwrap();
        assert_eq!(config.url_scheme, scheme);
    }
}

#[test]
fn a_missing_or_invalid_url_scheme_is_refused() {
    let icon = png(512, 512);
    for scheme in [
        "",
        "url_scheme = 3",
        "url_scheme = \"\"",
        "url_scheme = \"Riso\"",
        "url_scheme = \"1riso\"",
        "url_scheme = \"+riso\"",
        "url_scheme = \"riso_app\"",
        "url_scheme = \"riso user\"",
        "url_scheme = \"riso:\"",
        "url_scheme = \"risó\"",
    ] {
        refused(config_with_scheme(ICON, Some(&icon), scheme), "url_scheme");
    }
}

#[test]
fn a_missing_or_invalid_identifier_is_refused() {
    let icon = png(1024, 1024);
    refused(config("icon = \"icon.png\"\n", Some(&icon)), "identifier");
    for identifier in ["sample", "com..example", "-com.example", "com.exa_mple"] {
        refused(
            config(
                &format!("identifier = \"{identifier}\"\nicon = \"icon.png\"\n"),
                Some(&icon),
            ),
            "identifier",
        );
    }
}

#[test]
fn a_missing_icon_key_or_file_is_refused_as_missing() {
    refused(config(IDENTIFIER, Some(&png(1024, 1024))), "icon");
    let error = refused(config(ICON, None), "icon");
    assert!(error.contains("missing or unreadable"), "{error}");
}

#[test]
fn an_absolute_icon_path_is_refused() {
    let (folder, _) = config("", Some(&png(1024, 1024)));
    let absolute = folder.path().join("icon.png");
    let error = refused(
        config(
            &format!("{IDENTIFIER}icon = {:?}\n", absolute.display().to_string()),
            None,
        ),
        "icon",
    );
    assert!(error.contains("relative"), "{error}");
}

#[test]
fn an_icon_that_is_not_a_valid_square_png_of_512_is_refused() {
    let mut bad_crc = png(1024, 1024);
    bad_crc[29] ^= 0xff;
    let mut old_stub = SIGNATURE.to_vec();
    old_stub.extend(13u32.to_be_bytes());
    old_stub.extend(b"IHDR");
    old_stub.extend(1024u32.to_be_bytes());
    old_stub.extend(1024u32.to_be_bytes());
    for icon in [
        b"not a png at all, just some text".to_vec(),
        png(1024, 1024)[..20].to_vec(),
        bad_crc,
        png_header(1024, 1024),
        old_stub,
        png(0, 0),
        png(1 << 31, 1 << 31),
    ] {
        let error = refused(config(ICON, Some(&icon)), "icon");
        assert!(error.contains("not a valid PNG"), "{error}");
    }
    for icon in [png(1024, 512), png(256, 256)] {
        let error = refused(config(ICON, Some(&icon)), "icon");
        assert!(error.contains("square and at least 512"), "{error}");
    }
}
