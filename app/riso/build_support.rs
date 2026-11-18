//! Reads and checks the product config, for `build.rs` and its test.

use std::path::{Path, PathBuf};

/// The product config's values, checked.
#[derive(Debug)]
pub struct ProductConfig {
    pub name: String,
    pub version: String,
    pub identifier: String,
    /// The icon's absolute path.
    pub icon: PathBuf,
}

/// Reads the product config at `path`. An error names the file and, when
/// one value is at fault, its key in backticks.
pub fn read_product_config(path: &Path) -> Result<ProductConfig, String> {
    let file = path.display();
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read product config {file}: {error}"))?;
    let config: toml::Table =
        toml::from_str(&text).map_err(|error| format!("invalid product config {file}: {error}"))?;
    let string = |key: &str| -> Result<String, String> {
        let value = config
            .get(key)
            .and_then(toml::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("product config {file}: `{key}` must be a non-empty string"))?;
        if value.contains(['\r', '\n']) {
            return Err(format!(
                "product config {file}: `{key}` must not contain line breaks"
            ));
        }
        Ok(value.to_owned())
    };
    let name = string("name")?;
    let version = string("version")?;
    let identifier = string("identifier")?;
    if !is_identifier(&identifier) {
        return Err(format!(
            "product config {file}: `identifier` {identifier:?} must be at least two labels \
             separated by dots, each of ASCII letters, digits, and `-`, not starting or ending \
             with `-`"
        ));
    }
    let icon = string("icon")?;
    if Path::new(&icon).is_absolute() {
        return Err(format!(
            "product config {file}: `icon` {icon:?} must be relative to the config file"
        ));
    }
    let icon = path.parent().unwrap_or(Path::new("")).join(&icon);
    let bytes = std::fs::read(&icon).map_err(|error| {
        format!(
            "product config {file}: `icon` {} is missing or unreadable: {error}",
            icon.display()
        )
    })?;
    let icon = icon.canonicalize().map_err(|error| {
        format!(
            "product config {file}: `icon` {} is missing or unreadable: {error}",
            icon.display()
        )
    })?;
    let (width, height) = png_size(&bytes).ok_or_else(|| {
        format!(
            "product config {file}: `icon` {} is not a valid PNG",
            icon.display()
        )
    })?;
    if width != height || width < 512 {
        return Err(format!(
            "product config {file}: `icon` {} is {width} × {height}; it must be square and at \
             least 512 × 512",
            icon.display()
        ));
    }
    Ok(ProductConfig {
        name,
        version,
        identifier,
        icon,
    })
}

/// At least two labels separated by dots, each of ASCII letters, digits, and
/// `-`, not starting or ending with `-`, as macOS asks of a bundle identifier.
fn is_identifier(identifier: &str) -> bool {
    let mut labels = 0;
    identifier.split('.').all(|label| {
        labels += 1;
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    }) && labels >= 2
}

/// The width and height of a PNG, or `None` when `bytes` are not one: the
/// signature, a first chunk `IHDR` of 13 bytes whose CRC matches and whose
/// sizes are within PNG's range, and chunks that run whole up to `IEND`.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    const MAX: u32 = (1 << 31) - 1;
    let number = |at: usize| -> Option<u32> {
        Some(u32::from_be_bytes(
            bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
        ))
    };
    if bytes.get(..8)? != SIGNATURE || number(8)? != 13 || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    if crc32(bytes.get(12..29)?) != number(29)? {
        return None;
    }
    let (width, height) = (number(16)?, number(20)?);
    if !(1..=MAX).contains(&width) || !(1..=MAX).contains(&height) {
        return None;
    }
    let mut at = 8;
    loop {
        let length = usize::try_from(number(at)?).ok()?;
        let kind = bytes.get(at + 4..at + 8)?;
        let end = at.checked_add(12)?.checked_add(length)?;
        if end > bytes.len() {
            return None;
        }
        if kind == b"IEND" {
            return Some((width, height));
        }
        at = end;
    }
}

/// The CRC-32 that PNG chunks carry (ISO-HDLC, polynomial `0xEDB88320`).
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
