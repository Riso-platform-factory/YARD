use crate::Extension;
use gpui::App;
use std::collections::HashSet;
#[cfg(windows)]
use std::path::Component;
use std::path::{Path, PathBuf};
use std::{fmt, fs};

pub const MAX_URL_LEN: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalOpenError {
    InvalidUrl,
    UrlTooLong,
    SchemeNotAllowed,
    InvalidPath,
    NotFound,
    NotAFile,
    Executable,
}

impl fmt::Display for ExternalOpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidUrl => "invalid URL",
            Self::UrlTooLong => "URL too long",
            Self::SchemeNotAllowed => "scheme not allowed",
            Self::InvalidPath => "invalid path",
            Self::NotFound => "path not found",
            Self::NotAFile => "not a file",
            Self::Executable => "executable file refused",
        })
    }
}

impl std::error::Error for ExternalOpenError {}

impl Extension {
    /// Checks a URL and hands it off to the system's registered handler.
    pub fn open_url(&self, url: &str, cx: &App) -> Result<(), ExternalOpenError> {
        let _group = tracing::info_span!(
            "external-open",
            extension = self.name.as_str(),
            kind = "url",
            scheme = logged_scheme(url),
        )
        .entered();
        let url = check_url(url).inspect_err(refused)?;
        let host = url.host_str();
        cx.open_url(url.as_str());
        tracing::debug!(host, "handed off");
        Ok(())
    }

    /// Checks a path and asks the file manager to reveal it.
    pub fn reveal_path(&self, path: &Path, cx: &App) -> Result<(), ExternalOpenError> {
        let _group = tracing::info_span!(
            "external-open",
            extension = self.name.as_str(),
            kind = "reveal"
        )
        .entered();
        let path = check_reveal(path).inspect_err(refused)?;
        cx.reveal_path(&path);
        tracing::debug!("handed off");
        Ok(())
    }

    /// Checks a file and hands its resolved path off to the system's handler.
    pub fn open_path(&self, path: &Path, cx: &App) -> Result<(), ExternalOpenError> {
        let _group = tracing::info_span!(
            "external-open",
            extension = self.name.as_str(),
            kind = "open"
        )
        .entered();
        let path = check_open(path).inspect_err(refused)?;
        cx.open_with_system(&path);
        tracing::debug!("handed off");
        Ok(())
    }
}

fn refused(error: &ExternalOpenError) {
    tracing::info!(reason = %error, "refused");
}

fn logged_scheme(url: &str) -> Option<&'static str> {
    if url.len() > MAX_URL_LEN {
        return None;
    }
    let (scheme, _) = url.split_once(':')?;
    const SCHEMES: &[&str] = &[
        "http",
        "https",
        "mailto",
        "file",
        "javascript",
        "data",
        "vbscript",
        "ftp",
        "ms-msdt",
        "search-ms",
    ];
    let mut bytes = scheme.bytes();
    (bytes.next()?.is_ascii_alphabetic()
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || b"+-.".contains(&byte)))
    .then(|| {
        SCHEMES
            .iter()
            .copied()
            .find(|name| scheme.eq_ignore_ascii_case(name))
            .unwrap_or("other")
    })
}

fn escaped_byte(digits: &[u8]) -> Option<u8> {
    let [high, low] = *digits else { return None };
    Some(((high as char).to_digit(16)? * 16 + (low as char).to_digit(16)?) as u8)
}

fn valid_escapes(text: &str, allow_line_breaks: bool) -> bool {
    let bytes = text.as_bytes();
    bytes.iter().enumerate().all(|(index, &byte)| {
        byte != b'%'
            || bytes
                .get(index + 1..index + 3)
                .and_then(escaped_byte)
                .is_some_and(|value| {
                    !value.is_ascii_control()
                        || (allow_line_breaks && matches!(value, b'\r' | b'\n'))
                })
    })
}

fn valid_url_escapes(raw: &str) -> bool {
    let (scheme, rest) = raw.split_once(':').unwrap_or(("", raw));
    if !scheme.eq_ignore_ascii_case("mailto") {
        return valid_escapes(raw, false);
    }

    let (address_and_query, fragment) = rest.split_once('#').unwrap_or((rest, ""));
    let (address, query) = address_and_query
        .split_once('?')
        .unwrap_or((address_and_query, ""));
    valid_escapes(address, false)
        && valid_escapes(fragment, false)
        && query.split('&').all(|field| {
            let (name, value) = field.split_once('=').unwrap_or((field, ""));
            valid_escapes(name, false) && valid_escapes(value, name.eq_ignore_ascii_case("body"))
        })
}

fn percent_decode(text: &str) -> Option<Vec<u8>> {
    let mut bytes = text.bytes();
    let mut decoded = Vec::with_capacity(text.len());
    while let Some(byte) = bytes.next() {
        decoded.push(if byte == b'%' {
            escaped_byte(&[bytes.next()?, bytes.next()?])?
        } else {
            byte
        });
    }
    Some(decoded)
}

fn valid_address_part(part: &str) -> bool {
    percent_decode(part).is_some_and(|bytes| {
        std::str::from_utf8(&bytes).is_ok_and(|text| {
            !text.is_empty()
                && !text.chars().any(|character| {
                    character == '@' || character.is_whitespace() || character.is_control()
                })
        })
    })
}

fn valid_recipients(recipients: &str) -> bool {
    recipients.split(',').all(|recipient| {
        recipient
            .split_once('@')
            .is_some_and(|(local, domain)| valid_address_part(local) && valid_address_part(domain))
    })
}

fn valid_mailto(url: &url::Url) -> bool {
    if !url.cannot_be_a_base() || !valid_recipients(url.path()) {
        return false;
    }

    let mut names = HashSet::new();
    url.query().is_none_or(|query| {
        query
            .split('&')
            .filter(|field| !field.is_empty())
            .all(|field| {
                let (name, value) = field.split_once('=').unwrap_or((field, ""));
                let Some(mut name) = percent_decode(name) else {
                    return false;
                };
                // Encoded spellings can designate the same field in the mail client.
                name.make_ascii_lowercase();
                let recipient_field = matches!(name.as_slice(), b"to" | b"cc" | b"bcc");
                names.insert(name) && (!recipient_field || valid_recipients(value))
            })
    })
}

fn url_authority(rest: &str) -> Option<(&str, &str)> {
    let after_slashes = rest.strip_prefix("//")?;
    let end = after_slashes
        .find(['/', '?', '#'])
        .unwrap_or(after_slashes.len());
    Some(after_slashes.split_at(end))
}

fn valid_host_brackets(authority: &str, suffix: &str) -> bool {
    let Some((host, port)) = authority
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
    else {
        return false;
    };
    host.parse::<std::net::Ipv6Addr>().is_ok()
        && (port.is_empty()
            || port
                .strip_prefix(':')
                .is_some_and(|port| port.bytes().all(|byte| byte.is_ascii_digit())))
        && !suffix.contains(['[', ']'])
}

fn check_url(raw: &str) -> Result<url::Url, ExternalOpenError> {
    use ExternalOpenError::{InvalidUrl, SchemeNotAllowed, UrlTooLong};

    if raw.len() > MAX_URL_LEN {
        return Err(UrlTooLong);
    }
    if raw.is_empty()
        || raw.chars().any(|character| {
            character.is_whitespace() || character.is_control() || "<>\"{}|\\^`".contains(character)
        })
        || !valid_url_escapes(raw)
    {
        return Err(InvalidUrl);
    }
    let (_, rest) = raw.split_once(':').ok_or(InvalidUrl)?;
    let authority = url_authority(rest);
    if raw.contains(['[', ']'])
        && !authority.is_some_and(|(authority, suffix)| valid_host_brackets(authority, suffix))
    {
        return Err(InvalidUrl);
    }
    let url = url::Url::parse(raw).map_err(|_| InvalidUrl)?;
    if url.as_str().len() > MAX_URL_LEN {
        return Err(UrlTooLong);
    }

    match url.scheme() {
        "http" | "https" => {
            // The parser can invent a host when the raw authority is empty.
            let (authority, _) = authority.ok_or(InvalidUrl)?;
            if authority.is_empty() || authority.contains('@') || !url.has_host() {
                return Err(InvalidUrl);
            }
        }
        "mailto" => {
            if !valid_mailto(&url) {
                return Err(InvalidUrl);
            }
        }
        _ => return Err(SchemeNotAllowed),
    }
    Ok(url)
}

fn check_path(path: &Path) -> Result<(), ExternalOpenError> {
    use ExternalOpenError::InvalidPath;

    let text = path.to_str().ok_or(InvalidPath)?;
    if !path.is_absolute()
        || path.parent().is_none()
        || text.chars().any(char::is_control)
        || text
            .split(std::path::is_separator)
            .any(|part| matches!(part, "." | ".."))
    {
        return Err(InvalidPath);
    }
    #[cfg(windows)]
    check_windows_path(path, text)?;
    Ok(())
}

#[cfg(windows)]
fn check_windows_path(path: &Path, text: &str) -> Result<(), ExternalOpenError> {
    use ExternalOpenError::InvalidPath;
    use std::path::Prefix;
    use windows::{Win32::Storage::FileSystem::GetDriveTypeW, core::PCWSTR};

    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return Err(InvalidPath);
    };
    let Prefix::Disk(drive) = prefix.kind() else {
        return Err(InvalidPath);
    };
    if text[2..].split(['\\', '/']).any(|part| {
        if part.contains(':') || part.ends_with(['.', ' ']) {
            return true;
        }
        let stem = part.split_once('.').map_or(part, |(stem, _)| stem);
        let stem = stem.trim_end_matches(' ').to_ascii_uppercase();
        matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(suffix.as_bytes(), [b'0'..=b'9']) || matches!(suffix, "¹" | "²" | "³")
            })
    }) {
        return Err(InvalidPath);
    }

    let root = [u16::from(drive), b':' as u16, b'\\' as u16, 0];
    // SAFETY: `root` is a NUL-terminated UTF-16 drive root and lives through the call.
    let drive_type = unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) };
    // DRIVE_REMOVABLE, DRIVE_FIXED, DRIVE_CDROM, and DRIVE_RAMDISK.
    if !matches!(drive_type, 2 | 3 | 5 | 6) {
        return Err(InvalidPath);
    }
    Ok(())
}

fn resolve_path(path: &Path) -> Result<(PathBuf, fs::Metadata), ExternalOpenError> {
    use ExternalOpenError::NotFound;

    check_path(path)?;
    let resolved = fs::canonicalize(path).map_err(|_| NotFound)?;
    #[cfg(windows)]
    let plain = plain_resolved_path(&resolved);
    #[cfg(not(windows))]
    let plain = &resolved;
    check_path(plain)?;
    let metadata = fs::metadata(&resolved).map_err(|_| NotFound)?;
    #[cfg(windows)]
    let resolved = plain.to_path_buf();
    Ok((resolved, metadata))
}

#[cfg(windows)]
fn plain_resolved_path(path: &Path) -> &Path {
    use std::path::Prefix;

    if let Some(Component::Prefix(prefix)) = path.components().next()
        && matches!(prefix.kind(), Prefix::VerbatimDisk(_))
        && let Some(plain) = path.to_str().and_then(|text| text.strip_prefix(r"\\?\"))
    {
        return Path::new(plain);
    }
    path
}

fn path_for_gpui(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    if let Some(text) = path.to_str().filter(|text| text.contains('/')) {
        return PathBuf::from(text.replace('/', "\\"));
    }
    path
}

fn check_reveal(path: &Path) -> Result<PathBuf, ExternalOpenError> {
    let (_, metadata) = resolve_path(path)?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(ExternalOpenError::NotAFile);
    }
    Ok(path_for_gpui(path.to_path_buf()))
}

fn check_open(path: &Path) -> Result<PathBuf, ExternalOpenError> {
    use ExternalOpenError::{Executable, NotAFile};

    let (resolved, metadata) = resolve_path(path)?;
    if !metadata.is_file() {
        // TODO(external-open-folder): open a folder in the file manager. Needs telling a macOS bundle from a folder (NSWorkspace isFilePackageAtPath:), YARD#19.
        return Err(NotAFile);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if metadata.permissions().mode() & 0o111 != 0 {
            return Err(Executable);
        }
    }
    if executable_name(path) || executable_name(&resolved) {
        return Err(Executable);
    }
    Ok(path_for_gpui(resolved))
}

fn executable_name(path: &Path) -> bool {
    const EXTENSIONS: &[&str] = &[
        "ade",
        "adp",
        "app",
        "application",
        "appref-ms",
        "asp",
        "aspx",
        "asx",
        "bas",
        "bat",
        "bgi",
        "cer",
        "chm",
        "cmd",
        "cnt",
        "com",
        "cpl",
        "crt",
        "csh",
        "der",
        "diagcab",
        "exe",
        "fxp",
        "gadget",
        "grp",
        "hlp",
        "hpj",
        "hta",
        "htc",
        "inf",
        "ins",
        "isp",
        "its",
        "jar",
        "jnlp",
        "js",
        "jse",
        "ksh",
        "lnk",
        "mad",
        "maf",
        "mag",
        "mam",
        "maq",
        "mar",
        "mas",
        "mat",
        "mau",
        "mav",
        "maw",
        "mcf",
        "mda",
        "mdb",
        "mde",
        "mdt",
        "mdw",
        "mdz",
        "msc",
        "msh",
        "msh1",
        "msh2",
        "mshxml",
        "msh1xml",
        "msh2xml",
        "msi",
        "msp",
        "mst",
        "msu",
        "ops",
        "osd",
        "pcd",
        "pif",
        "pl",
        "plg",
        "prf",
        "prg",
        "printerexport",
        "ps1",
        "ps1xml",
        "ps2",
        "ps2xml",
        "psc1",
        "psc2",
        "psd1",
        "psdm1",
        "psm1",
        "pst",
        "py",
        "pyc",
        "pyo",
        "pyw",
        "pyz",
        "pyzw",
        "reg",
        "scf",
        "scr",
        "sct",
        "settingcontent-ms",
        "shb",
        "shs",
        "theme",
        "tmp",
        "url",
        "vb",
        "vbe",
        "vbp",
        "vbs",
        "vhd",
        "vhdx",
        "vsmacros",
        "vsw",
        "webpnp",
        "website",
        "ws",
        "wsb",
        "wsc",
        "wsf",
        "wsh",
        "xbap",
        "xll",
        "xnk",
        "appx",
        "appxbundle",
        "msix",
        "msixbundle",
        "appinstaller",
        "library-ms",
        "searchconnector-ms",
        "iso",
        "img",
        "command",
        "tool",
        "pkg",
        "mpkg",
        "dmg",
        "scpt",
        "scptd",
        "applescript",
        "action",
        "workflow",
        "terminal",
        "webloc",
        "inetloc",
        "fileloc",
        "ftploc",
        "mailloc",
        "prefpane",
        "desktop",
        "appimage",
        "sh",
        "bash",
        "zsh",
        "fish",
        "run",
        "bin",
        "deb",
        "rpm",
        "flatpakref",
        "rb",
    ];
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            EXTENSIONS
                .iter()
                .any(|blocked| extension.eq_ignore_ascii_case(blocked))
        })
}

#[cfg(test)]
mod tests;
