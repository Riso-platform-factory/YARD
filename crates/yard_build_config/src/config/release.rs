//! The `[release]` tables: how each system's package is signed and wrapped.

use std::path::{Path, PathBuf};

/// `[release.macos]`. Absent, the app is signed ad hoc and has no disk
/// image.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MacosRelease {
    /// Whether a disk image is made beside the zip.
    pub dmg: bool,
    /// The entitlements file's absolute, canonical path, `None` when the
    /// config names none.
    pub entitlements: Option<PathBuf>,
    /// `[release.macos.signing]`, `None` for an ad hoc signature.
    pub signing: Option<MacosSigning>,
}

/// `[release.macos.signing]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacosSigning {
    /// The signing identity's name, as the keychain knows it.
    pub identity: String,
    /// Whether the app, and its disk image, are notarized and stapled.
    pub notarize: bool,
}

/// `[release.windows]`. Absent, nothing is signed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WindowsRelease {
    /// `[release.windows.signing]`, `None` when nothing is signed.
    pub signing: Option<WindowsSigning>,
}

/// `[release.windows.signing]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsSigning {
    /// The RFC 3161 timestamp server.
    pub timestamp_url: String,
}

/// The timestamp server when `[release.windows.signing]` names none.
pub const DEFAULT_TIMESTAMP_URL: &str = "http://timestamp.digicert.com";

/// One table of the config, with its dotted name for errors.
struct Table<'a> {
    file: &'a Path,
    name: String,
    table: &'a toml::Table,
}

impl<'a> Table<'a> {
    /// The table `value`, named `name`, holding no key but `keys`.
    fn new(
        file: &'a Path,
        name: String,
        value: &'a toml::Value,
        keys: &[&str],
    ) -> Result<Self, String> {
        let table = value.as_table().ok_or_else(|| {
            format!(
                "product config {}: `{name}` must be a table",
                file.display()
            )
        })?;
        if let Some(key) = table
            .keys()
            .filter(|key| !keys.contains(&key.as_str()))
            .min()
        {
            return Err(format!(
                "product config {}: unknown key `{name}.{key}`",
                file.display()
            ));
        }
        Ok(Self { file, name, table })
    }

    fn key(&self, key: &str) -> String {
        format!("{}.{key}", self.name)
    }

    fn table(&self, key: &str, keys: &[&str]) -> Result<Option<Table<'a>>, String> {
        self.table
            .get(key)
            .map(|value| Table::new(self.file, self.key(key), value, keys))
            .transpose()
    }

    fn bool(&self, key: &str, default: bool) -> Result<bool, String> {
        match self.table.get(key) {
            None => Ok(default),
            Some(value) => value.as_bool().ok_or_else(|| {
                format!(
                    "product config {}: `{}` must be true or false",
                    self.file.display(),
                    self.key(key)
                )
            }),
        }
    }

    fn string(&self, key: &str) -> Result<Option<String>, String> {
        let Some(value) = self.table.get(key) else {
            return Ok(None);
        };
        let value = value
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                format!(
                    "product config {}: `{}` must be a non-empty string",
                    self.file.display(),
                    self.key(key)
                )
            })?;
        if value.contains(['\r', '\n']) {
            return Err(format!(
                "product config {}: `{}` must not contain line breaks",
                self.file.display(),
                self.key(key)
            ));
        }
        Ok(Some(value.to_owned()))
    }
}

/// Reads `[release]` of the config at `path`: only the tables `macos`,
/// `windows`, and `linux`, each with only its own keys.
pub(super) fn read(
    path: &Path,
    release: Option<&toml::Value>,
) -> Result<(MacosRelease, WindowsRelease), String> {
    let file = path.display();
    let Some(release) = release else {
        return Ok(Default::default());
    };
    let release = Table::new(
        path,
        "release".into(),
        release,
        &["macos", "windows", "linux"],
    )?;
    release.table("linux", &[])?;

    let mut macos = MacosRelease::default();
    if let Some(table) = release.table("macos", &["dmg", "entitlements", "signing"])? {
        macos.dmg = table.bool("dmg", false)?;
        if let Some(entitlements) = table.string("entitlements")? {
            macos.entitlements = Some(entitlements_path(path, &table, &entitlements)?);
        }
        if let Some(signing) = table.table("signing", &["identity", "notarize"])? {
            let identity = signing.string("identity")?.ok_or_else(|| {
                format!(
                    "product config {file}: `{}` must be a non-empty string",
                    signing.key("identity")
                )
            })?;
            macos.signing = Some(MacosSigning {
                identity,
                notarize: signing.bool("notarize", true)?,
            });
        }
    }

    let mut windows = WindowsRelease::default();
    if let Some(table) = release.table("windows", &["signing"])?
        && let Some(signing) = table.table("signing", &["timestamp_url"])?
    {
        let timestamp_url = signing
            .string("timestamp_url")?
            .unwrap_or_else(|| DEFAULT_TIMESTAMP_URL.to_owned());
        match super::split_url(&timestamp_url) {
            Ok(("http" | "https", _)) => {}
            Ok(_) => {
                return Err(format!(
                    "product config {file}: `{}` {timestamp_url:?} must be an http:// or \
                     https:// URL",
                    signing.key("timestamp_url")
                ));
            }
            Err(reason) => {
                return Err(format!(
                    "product config {file}: `{}` {timestamp_url:?} {reason}",
                    signing.key("timestamp_url")
                ));
            }
        }
        windows.signing = Some(WindowsSigning { timestamp_url });
    }
    Ok((macos, windows))
}

/// The entitlements file, relative to the config, made absolute and
/// canonical as the icon is.
fn entitlements_path(path: &Path, table: &Table, entitlements: &str) -> Result<PathBuf, String> {
    let file = path.display();
    let key = table.key("entitlements");
    if Path::new(entitlements).is_absolute() {
        return Err(format!(
            "product config {file}: `{key}` {entitlements:?} must be relative to the config file"
        ));
    }
    let joined = path.parent().unwrap_or(Path::new("")).join(entitlements);
    joined
        .canonicalize()
        .ok()
        .filter(|canonical| canonical.is_file())
        .ok_or_else(|| {
            format!(
                "product config {file}: `{key}` {} is missing or not a file",
                joined.display()
            )
        })
}
