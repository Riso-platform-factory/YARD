//! Authenticode signing for `bundle windows`, when the config has
//! `[release.windows.signing]`, without any certificate store: the `.pfx`
//! from the environment is written to a private folder of the user's
//! temporary folder for the run only, and `signtool` signs with that file and
//! its password.
//!
//! The password is on `signtool`'s command line, where processes of the same
//! account and process-creation auditing can see it. It is also in the
//! environment of the PowerShell that reads the `.pfx`, of ISCC, and of
//! xtask's sign helper, which ISCC runs with that environment. It is not in
//! ISCC's arguments, the `.iss`, a log line, or an error. What signtool and
//! ISCC print is captured, and the password replaced with `***`, before it is
//! shown; the PowerShell's output is never shown.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;

use yard_build_config::BuildConfig;

use super::super::signing::{HELPER_VARIABLES, SECRET_VARIABLES, base64_decode};
use super::super::{Captured, Runner};
use super::WindowsHost;

/// The secrets of Windows signing, from the environment only.
#[derive(Clone, Default)]
pub struct WindowsSecrets {
    /// `WINDOWS_CERTIFICATE`: the code-signing certificate's `.pfx`, base64.
    pub certificate: Option<String>,
    /// `WINDOWS_CERTIFICATE_PASSWORD`.
    pub certificate_password: Option<String>,
}

impl WindowsSecrets {
    /// Reads the variables; an empty one counts as absent.
    pub fn from_env() -> Self {
        let var = |name| std::env::var(name).ok().filter(|value| !value.is_empty());
        Self {
            certificate: var("WINDOWS_CERTIFICATE"),
            certificate_password: var("WINDOWS_CERTIFICATE_PASSWORD"),
        }
    }
}

/// Says which secrets are set, never their values.
impl fmt::Debug for WindowsSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowsSecrets")
            .field("certificate", &self.certificate.is_some())
            .field("certificate_password", &self.certificate_password.is_some())
            .finish()
    }
}

/// The certificate and its password, checked before anything is built.
pub(super) struct Certificate {
    pfx: Vec<u8>,
    password: String,
    timestamp_url: String,
}

/// Checks the secrets that `[release.windows.signing]` needs; `None` without
/// the table. An error names the variable at fault, never a value.
pub(super) fn certificate(
    product: &BuildConfig,
    secrets: &WindowsSecrets,
) -> Result<Option<Certificate>, String> {
    let Some(signing) = &product.windows.signing else {
        tracing::debug!("no [release.windows.signing]: not signed");
        return Ok(None);
    };
    let missing: Vec<&str> = [
        ("WINDOWS_CERTIFICATE", &secrets.certificate),
        (
            "WINDOWS_CERTIFICATE_PASSWORD",
            &secrets.certificate_password,
        ),
    ]
    .into_iter()
    .filter(|(_, value)| value.is_none())
    .map(|(name, _)| name)
    .collect();
    if !missing.is_empty() {
        return Err(format!(
            "[release.windows.signing] needs {} in the environment",
            missing.join(", ")
        ));
    }
    if secrets.certificate_password.as_deref() == Some("") {
        return Err("WINDOWS_CERTIFICATE_PASSWORD is empty".into());
    }
    let pfx = base64_decode(secrets.certificate.as_deref().unwrap_or_default())
        .ok_or("WINDOWS_CERTIFICATE is not valid base64")?;
    tracing::debug!(timestamp_url = signing.timestamp_url, "signing certificate");
    Ok(Some(Certificate {
        pfx,
        password: secrets.certificate_password.clone().unwrap_or_default(),
        timestamp_url: signing.timestamp_url.clone(),
    }))
}

/// The programs that sign, each by its absolute path: `signtool.exe`,
/// Windows PowerShell, and xtask itself, which Inno Setup runs to sign.
pub(super) struct SignTools {
    signtool: PathBuf,
    powershell: PathBuf,
    xtask: PathBuf,
}

impl SignTools {
    pub(super) fn new(host: &WindowsHost) -> Result<Self, String> {
        let signtool = signtool(host)?;
        let powershell = host.system_tool(&["WindowsPowerShell", "v1.0", "powershell.exe"])?;
        let xtask = host
            .xtask
            .clone()
            .filter(|xtask| xtask.is_absolute())
            .ok_or("xtask's own executable was not found, and Inno Setup runs it to sign")?;
        tracing::debug!(signtool = %signtool.display(), "signtool");
        Ok(Self {
            signtool,
            powershell,
            xtask,
        })
    }
}

/// `signtool.exe` on `PATH`, else the highest version in
/// `%ProgramFiles(x86)%\Windows Kits\10\bin\<version>\x64`, made absolute
/// once, so xtask and Inno Setup run the same file whatever their folder.
fn signtool(host: &WindowsHost) -> Result<PathBuf, String> {
    let on_path = host
        .path
        .iter()
        .flat_map(std::env::split_paths)
        .map(|folder| folder.join("signtool.exe"))
        .find(|path| path.is_file());
    let found = match on_path {
        Some(found) => Some(found),
        None => {
            let kits = host.program_files_x86.as_ref().map(|folder| {
                Path::new(folder)
                    .join("Windows Kits")
                    .join("10")
                    .join("bin")
            });
            let mut versions: Vec<(Vec<u32>, PathBuf)> = kits
                .iter()
                .filter_map(|bin| fs::read_dir(bin).ok())
                .flatten()
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    let name = entry.file_name().into_string().ok()?;
                    let version = name
                        .split('.')
                        .map(|part| part.parse::<u32>().ok())
                        .collect::<Option<Vec<u32>>>()?;
                    let tool = entry.path().join("x64").join("signtool.exe");
                    tool.is_file().then_some((version, tool))
                })
                .collect();
            versions.sort();
            versions.pop().map(|(_, tool)| tool)
        }
    };
    let found = found.ok_or(
        "signtool.exe was not found: it is neither on PATH nor in %ProgramFiles(x86)%\\Windows Kits\\10\\bin\\<version>\\x64",
    )?;
    std::path::absolute(&found)
        .map_err(|error| format!("could not make {} absolute: {error}", found.display()))
}

/// A Windows command without any secret or signing context in its
/// environment: neither the signing variables nor any `XTASK_SIGN_*`, which
/// each command that needs one gets back by name.
pub(super) fn command(program: &Path) -> Command {
    let mut command = Command::new(program);
    for name in SECRET_VARIABLES
        .iter()
        .copied()
        .map(OsString::from)
        .chain(sign_variables())
    {
        command.env_remove(name);
    }
    command
}

/// The sign helper's variables, and any other variable of this process
/// whose name starts with `XTASK_SIGN_`.
pub(super) fn sign_variables() -> Vec<OsString> {
    let mut names: Vec<OsString> = HELPER_VARIABLES.iter().map(OsString::from).collect();
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("XTASK_SIGN_") && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The variables that give xtask's sign helper what it signs with, set only
/// on ISCC, which runs the helper, and the two that PowerShell is given.
const SIGNTOOL_VARIABLE: &str = "XTASK_SIGN_SIGNTOOL";
const POWERSHELL_VARIABLE: &str = "XTASK_SIGN_POWERSHELL";
const PFX_VARIABLE: &str = "XTASK_SIGN_PFX";
const PASSWORD_VARIABLE: &str = "XTASK_SIGN_PASSWORD";
const TIMESTAMP_VARIABLE: &str = "XTASK_SIGN_TIMESTAMP_URL";
const THUMBPRINT_VARIABLE: &str = "XTASK_SIGN_THUMBPRINT";
const USAGE_VARIABLE: &str = "XTASK_SIGN_USAGE";
const FILE_VARIABLE: &str = "XTASK_SIGN_FILE";

/// The code-signing extended key usage.
const CODE_SIGNING: &str = "1.3.6.1.5.5.7.3.3";

/// Loads the `.pfx` at `$env:XTASK_SIGN_PFX` in memory only
/// (`EphemeralKeySet`), and prints how many of its certificates have a
/// private key and the code-signing usage, and that one's thumbprint. A
/// failure prints only `error=<the innermost exception's type>`, and the
/// script ends with code 1.
const READ_LEAF: &str = r"$ErrorActionPreference = 'Stop'
try { $flags = [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::EphemeralKeySet; $collection = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2Collection; $collection.Import($env:XTASK_SIGN_PFX, $env:XTASK_SIGN_PASSWORD, $flags); $signing = @($collection | Where-Object { $certificate = $_; $certificate.HasPrivateKey -and @($certificate.Extensions | Where-Object { $_ -is [System.Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension] } | ForEach-Object { $_.EnhancedKeyUsages } | Where-Object { $_.Value -eq $env:XTASK_SIGN_USAGE }).Count -gt 0 }) } catch { Write-Output ('error=' + $_.Exception.GetBaseException().GetType().FullName); exit 1 }
Write-Output ('count=' + $signing.Count)
if ($signing.Count -eq 1) { Write-Output ('thumbprint=' + $signing[0].Thumbprint) }
";

/// Prints what `Get-AuthenticodeSignature` reads in `$env:XTASK_SIGN_FILE`:
/// its status, its type, and the thumbprints of its signer and of its
/// timestamp.
const CHECK: &str = r"$ErrorActionPreference = 'Stop'
$signature = Get-AuthenticodeSignature -LiteralPath $env:XTASK_SIGN_FILE
Write-Output ('status=' + $signature.Status)
Write-Output ('type=' + $signature.SignatureType)
Write-Output ('signer=' + $signature.SignerCertificate.Thumbprint)
Write-Output ('timestamp=' + $signature.TimeStamperCertificate.Thumbprint)
";

/// What signs one file: the tools, the `.pfx` file and its password, the
/// timestamp server, and the thumbprint of the `.pfx`'s signing
/// certificate.
pub(super) struct Signer {
    signtool: PathBuf,
    powershell: PathBuf,
    pfx: PathBuf,
    password: String,
    timestamp_url: String,
    thumbprint: String,
}

impl Signer {
    /// Reads the signing certificate's thumbprint from the `.pfx` at `pfx`.
    pub(super) fn new(
        tools: &SignTools,
        certificate: &Certificate,
        pfx: &Path,
        runner: &mut dyn Runner,
    ) -> Result<Self, String> {
        let mut read = powershell(&tools.powershell, READ_LEAF);
        read.env(PFX_VARIABLE, pfx)
            .env(PASSWORD_VARIABLE, &certificate.password)
            .env(USAGE_VARIABLE, CODE_SIGNING);
        let unreadable = |reason: String| {
            format!(
                "could not read WINDOWS_CERTIFICATE with WINDOWS_CERTIFICATE_PASSWORD: {reason}"
            )
        };
        // What PowerShell printed is never shown: only the exception's type,
        // from the one field the script prints on a failure.
        let captured = runner
            .capture(&mut read)
            .map_err(|_| unreadable("Windows PowerShell could not run".into()))?;
        if !captured.success {
            let error = String::from_utf8_lossy(&captured.stdout)
                .lines()
                .find_map(|line| line.trim().strip_prefix("error="))
                .filter(|name| {
                    !name.is_empty()
                        && name
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+'))
                })
                .map(str::to_owned);
            return Err(unreadable(error.unwrap_or_else(|| {
                format!("powershell exited with {}", captured.status)
            })));
        }
        let thumbprint =
            leaf_thumbprint(&String::from_utf8_lossy(&captured.stdout)).map_err(unreadable)?;
        tracing::debug!(thumbprint, "signing certificate read");
        Ok(Self {
            signtool: tools.signtool.clone(),
            powershell: tools.powershell.clone(),
            pfx: pfx.to_owned(),
            password: certificate.password.clone(),
            timestamp_url: certificate.timestamp_url.clone(),
            thumbprint,
        })
    }

    /// The signer that xtask's sign helper rebuilds from its environment.
    fn from_env(var: &dyn Fn(&str) -> Option<OsString>) -> Result<Self, String> {
        let get = |name: &str| match var(name) {
            Some(value) if value.is_empty() => Err(format!(
                "{name} is empty: sign-windows-file runs only from bundle windows"
            )),
            Some(value) => Ok(value),
            None => Err(format!(
                "{name} is not set: sign-windows-file runs only from bundle windows"
            )),
        };
        let text = |name: &str| {
            get(name)?
                .into_string()
                .map_err(|_| format!("{name} is not valid Unicode"))
        };
        let thumbprint = text(THUMBPRINT_VARIABLE)?;
        if !is_thumbprint(&thumbprint) {
            return Err(format!("{THUMBPRINT_VARIABLE} is not a thumbprint"));
        }
        Ok(Self {
            signtool: get(SIGNTOOL_VARIABLE)?.into(),
            powershell: get(POWERSHELL_VARIABLE)?.into(),
            pfx: get(PFX_VARIABLE)?.into(),
            password: text(PASSWORD_VARIABLE)?,
            timestamp_url: text(TIMESTAMP_VARIABLE)?,
            thumbprint,
        })
    }

    /// Checks `file`'s signature, as [`check_authenticode`] does.
    pub(super) fn check(&self, file: &Path, runner: &mut dyn Runner) -> Result<(), String> {
        check_authenticode(&self.powershell, file, &self.thumbprint, runner)
    }

    /// Runs ISCC, which holds the password in its environment for the sign
    /// helper: what it prints is captured, then shown with the password
    /// hidden, so its own errors stay readable.
    pub(super) fn run_iscc(
        &self,
        iscc: &mut Command,
        runner: &mut dyn Runner,
    ) -> Result<(), String> {
        let captured = runner.capture(iscc)?;
        show(&captured, &self.password);
        if captured.success {
            Ok(())
        } else {
            Err(format!(
                "ISCC exited with {}; Inno Setup printed why above",
                captured.status
            ))
        }
    }

    /// Sets the variables of the sign helper on ISCC, the only program that
    /// gets them.
    pub(super) fn give_to(&self, iscc: &mut Command) {
        iscc.env(SIGNTOOL_VARIABLE, &self.signtool)
            .env(POWERSHELL_VARIABLE, &self.powershell)
            .env(PFX_VARIABLE, &self.pfx)
            .env(PASSWORD_VARIABLE, &self.password)
            .env(TIMESTAMP_VARIABLE, &self.timestamp_url)
            .env(THUMBPRINT_VARIABLE, &self.thumbprint);
    }

    /// Signs `file` with `signtool`, then checks the signature. What
    /// `signtool` prints is shown with the password hidden.
    pub(super) fn sign(&self, file: &Path, runner: &mut dyn Runner) -> Result<(), String> {
        let mut signtool = command(&self.signtool);
        signtool
            .args(["sign", "/fd", "sha256", "/tr"])
            .arg(&self.timestamp_url)
            .args(["/td", "sha256", "/f"])
            .arg(&self.pfx)
            .arg("/p")
            .arg(&self.password)
            .arg(file);
        let captured = runner.capture(&mut signtool)?;
        // signtool does not print the password; what it prints is still
        // shown with the password hidden, and the error is a fixed one.
        show(&captured, &self.password);
        if !captured.success {
            return Err(format!(
                "signtool exited with {} signing {}; its output is above",
                captured.status,
                file.display()
            ));
        }
        self.check(file, runner)
    }
}

/// The thumbprint in what [`READ_LEAF`] printed: exactly `count=1` and one
/// thumbprint, and nothing else.
fn leaf_thumbprint(printed: &str) -> Result<String, String> {
    let lines: Vec<&str> = printed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let count = lines
        .first()
        .and_then(|line| line.strip_prefix("count="))
        .and_then(|count| count.parse::<usize>().ok())
        .ok_or("PowerShell printed no count of signing certificates")?;
    if count != 1 {
        return Err(format!(
            "it holds {count} certificates with a private key and the code-signing usage, and signing needs exactly one"
        ));
    }
    match lines[1..] {
        [line] => line
            .strip_prefix("thumbprint=")
            .filter(|thumbprint| is_thumbprint(thumbprint))
            .map(str::to_ascii_uppercase)
            .ok_or_else(|| "PowerShell printed no thumbprint".to_owned()),
        _ => Err("PowerShell printed something other than one thumbprint".into()),
    }
}

/// Checks that `file` carries an Authenticode signature of the certificate
/// `thumbprint`, with a timestamp, whose status is `Valid`. Any other status,
/// such as `HashMismatch` for a file changed after signing, or
/// `UnknownError` for a certificate this machine does not trust, fails.
pub fn check_authenticode(
    powershell: &Path,
    file: &Path,
    thumbprint: &str,
    runner: &mut dyn Runner,
) -> Result<(), String> {
    let mut check = self::powershell(powershell, CHECK);
    check.env(FILE_VARIABLE, file);
    let printed = runner.output(&mut check)?;
    let printed = String::from_utf8_lossy(&printed);
    let value = |key: &str| {
        printed
            .lines()
            .find_map(|line| line.trim().strip_prefix(key)?.strip_prefix('='))
            .unwrap_or_default()
            .to_owned()
    };
    let (status, kind, signer, timestamp) = (
        value("status"),
        value("type"),
        value("signer"),
        value("timestamp"),
    );
    let file_name = file.display();
    if !signer.eq_ignore_ascii_case(thumbprint) {
        let signer = if signer.is_empty() { "nobody" } else { &signer };
        return Err(format!(
            "{file_name} is signed by {signer}, not by the certificate {thumbprint}"
        ));
    }
    if kind != "Authenticode" {
        return Err(format!(
            "{file_name} has a signature of type {kind:?}, not an Authenticode signature of its own"
        ));
    }
    if timestamp.is_empty() {
        return Err(format!("{file_name}'s signature has no timestamp"));
    }
    if status != "Valid" {
        return Err(format!(
            "{file_name}'s signature is {status}, not Valid; a certificate that no trusted root vouches for, such as a self-signed one, must be trusted on this machine"
        ));
    }
    tracing::debug!(file = %file_name, signer, timestamp, status, "signature checked");
    Ok(())
}

/// `cargo xtask sign-windows-file <file>`: what Inno Setup runs, through
/// `SignTool=`, to sign the installer and the uninstaller as the app was
/// signed, and to check the signature, with what `bundle windows` gave ISCC
/// in its environment (`var`).
pub fn sign_windows_file(
    file: &Path,
    var: &dyn Fn(&str) -> Option<OsString>,
    runner: &mut dyn Runner,
) -> Result<(), String> {
    let _group = tracing::info_span!("sign-windows-file", file = %file.display()).entered();
    let signer = Signer::from_env(var).inspect_err(|reason| {
        // Run by hand, without what `bundle windows` gives it: expected.
        tracing::info!("refused: {reason}");
    })?;
    signer
        .sign(file, runner)
        .inspect_err(|reason| log::error!("could not sign {}: {reason}", file.display()))
}

/// ISCC's `/S<name>=<command>`: the sign tool that runs xtask's sign helper.
/// Inno Setup reads `$f` as the file's quoted name, `$q` as a double quote,
/// and `$$` as a `$`; a `"` itself would not survive ISCC's own command line.
/// The command holds no secret and no path to the `.pfx`, since ISCC prints
/// the command it runs.
pub(super) fn iscc_sign_tool(tools: &SignTools) -> String {
    let inno = |value: &str| value.replace('$', "$$").replace('"', "$q");
    format!(
        "/S{SIGN_TOOL}=$q{}$q sign-windows-file $f",
        inno(&tools.xtask.to_string_lossy())
    )
}

/// The name of the sign tool that the `.iss` names in `SignTool=`.
pub(super) const SIGN_TOOL: &str = "xtask";

/// Shows on stderr what a program that saw the password printed, with the
/// password hidden.
fn show(captured: &Captured, password: &str) {
    let mut stderr = io::stderr();
    for bytes in [&captured.stdout, &captured.stderr] {
        let _ = stderr.write_all(hidden(&String::from_utf8_lossy(bytes), password).as_bytes());
    }
}

/// `text` with every occurrence of `password` replaced by `***`.
fn hidden(text: &str, password: &str) -> String {
    if password.is_empty() {
        text.to_owned()
    } else {
        text.replace(password, "***")
    }
}

/// Windows PowerShell running `script`, which takes its values from its
/// environment, so none is quoted into it.
fn powershell(program: &Path, script: &str) -> Command {
    let mut command = command(program);
    // Windows PowerShell builds its own module path. One that PowerShell 7
    // set, when xtask runs from it, hides the modules of Windows PowerShell,
    // such as the one of `Get-AuthenticodeSignature`.
    command.env_remove("PSModulePath");
    command
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(script.lines().collect::<Vec<_>>().join("; "));
    command
}

/// A SHA-1 thumbprint: 40 hexadecimal digits.
fn is_thumbprint(value: &str) -> bool {
    value.len() == 40 && value.chars().all(|c| c.is_ascii_hexdigit())
}

/// The folder that holds the `.pfx` for the run: made under the user's
/// temporary folder with a random name, only if it did not exist, open to
/// the user alone, and removed when this drops, on every path.
pub(super) struct PfxFolder {
    path: PathBuf,
    removed: bool,
}

impl PfxFolder {
    /// Makes the folder and writes `certificate`'s `.pfx` in it, as a new
    /// file, once the temporary folder's volume keeps access lists.
    pub(super) fn new(certificate: &Certificate) -> Result<(Self, PathBuf), String> {
        let temp = std::env::temp_dir();
        keeps_access_lists(&temp, &persistent_acls)?;
        let folder = loop {
            let path = temp.join(format!("xtask-signing-{}", random_name()));
            match private_folder(&path) {
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!(
                        "could not create a private folder in {}: {error}",
                        temp.display()
                    ));
                }
                Ok(()) => {
                    break Self {
                        path,
                        removed: false,
                    };
                }
            }
        };
        let pfx = folder.path.join("certificate.pfx");
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pfx)
            .and_then(|mut file| file.write_all(&certificate.pfx))
            .map_err(|error| format!("could not write the certificate's file: {error}"))?;
        tracing::debug!(folder = %folder.path.display(), "certificate file written");
        Ok((folder, pfx))
    }

    /// Removes the folder. It counts as removed only once it is gone.
    pub(super) fn remove(&mut self) -> Result<(), String> {
        if self.removed {
            return Ok(());
        }
        match fs::remove_dir_all(&self.path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                return Err(format!(
                    "could not remove the certificate's folder {}: {error}; delete it",
                    self.path.display()
                ));
            }
            _ => {}
        }
        self.removed = true;
        tracing::debug!(folder = %self.path.display(), "certificate file removed");
        Ok(())
    }
}

/// After a failure, an interrupt, or a panic, the folder is still removed; if
/// it cannot be, a warning names it.
impl Drop for PfxFolder {
    fn drop(&mut self) {
        if let Err(reason) = self.remove() {
            log::warn!("{reason}");
        }
    }
}

/// Refuses a temporary folder whose volume does not keep access lists, such
/// as FAT or exFAT, where the private folder's list would not hold.
/// `persistent` asks the volume (`persistent_acls`, or a test's).
pub(super) fn keeps_access_lists(
    temp: &Path,
    persistent: &dyn Fn(&Path) -> io::Result<bool>,
) -> Result<(), String> {
    match persistent(temp) {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!(
            "the temporary folder {} is on a volume that keeps no access lists, so the certificate's file could not be private; set TEMP to a folder on an NTFS volume",
            temp.display()
        )),
        Err(error) => Err(format!(
            "could not tell whether the temporary folder {} keeps access lists: {error}",
            temp.display()
        )),
    }
}

/// 32 hex digits from the system's randomness, which seeds the keys of
/// every `RandomState`.
fn random_name() -> String {
    use std::hash::{BuildHasher, Hasher};
    (0..2)
        .map(|_| {
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_usize(0);
            format!("{:016x}", hasher.finish())
        })
        .collect()
}

/// Creates `path`, failing if it exists, so that only the user may open it:
/// mode 0700.
#[cfg(unix)]
fn private_folder(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(path)
}

/// A Unix folder's mode always holds.
#[cfg(unix)]
fn persistent_acls(_: &Path) -> io::Result<bool> {
    Ok(true)
}

/// Elsewhere, nothing would keep the folder private: signing is refused.
#[cfg(not(any(unix, windows)))]
fn private_folder(_: &Path) -> io::Result<()> {
    Err(io::Error::other("this system cannot make a private folder"))
}

#[cfg(not(any(unix, windows)))]
fn persistent_acls(_: &Path) -> io::Result<bool> {
    Err(io::Error::other("signing needs Windows or a Unix system"))
}

/// Whether the volume of `path` keeps access lists (`FILE_PERSISTENT_ACLS`).
#[cfg(windows)]
fn persistent_acls(path: &Path) -> io::Result<bool> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows::Win32::Storage::FileSystem::{GetVolumeInformationW, GetVolumePathNameW};
    /// The volume flag of winnt.h.
    const FILE_PERSISTENT_ACLS: u32 = 0x8;
    use windows::core::PCWSTR;

    let error = |error: windows::core::Error| io::Error::from_raw_os_error(error.code().0 & 0xFFFF);
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut root = vec![0_u16; wide.len().max(260) + 1];
    // SAFETY: `wide` is NUL-terminated, and `root` is writable for its length.
    unsafe { GetVolumePathNameW(PCWSTR(wide.as_ptr()), &mut root) }.map_err(error)?;
    let mut flags = 0_u32;
    // SAFETY: `root` holds the NUL-terminated root that the call above wrote,
    // and `flags` receives the volume's flags.
    unsafe {
        GetVolumeInformationW(
            PCWSTR(root.as_ptr()),
            None,
            None,
            None,
            Some(&raw mut flags),
            None,
        )
    }
    .map_err(error)?;
    Ok(flags & FILE_PERSISTENT_ACLS != 0)
}

/// Creates `path`, failing if it exists, with a protected access list that
/// lets only the user in, inherited by what is made inside.
#[cfg(windows)]
fn private_folder(path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    };
    use windows::Win32::Security::{
        GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
        TokenUser,
    };
    use windows::Win32::Storage::FileSystem::CreateDirectoryW;
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::core::{HSTRING, PCWSTR, PWSTR};

    let error = |error: windows::core::Error| io::Error::from_raw_os_error(error.code().0 & 0xFFFF);
    let mut token = HANDLE::default();
    // SAFETY: the pseudo handle of this process may be queried, and `token`
    // receives a handle closed below.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.map_err(error)?;
    let mut len = 0;
    // SAFETY: asks only for the size, which fails with that size.
    let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut len) };
    // `u64`s, so the `TOKEN_USER` at the start is aligned.
    let mut buffer = vec![0_u64; (len as usize).div_ceil(8)];
    // SAFETY: `buffer` holds at least `len` writable bytes.
    let read = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            len,
            &mut len,
        )
    };
    // SAFETY: the token was opened above and is closed once.
    let _ = unsafe { CloseHandle(token) };
    read.map_err(error)?;
    // SAFETY: `GetTokenInformation` wrote a `TOKEN_USER` at the start of
    // `buffer`, whose SID points into `buffer`, alive below.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut string = PWSTR::null();
    // SAFETY: `sid` is valid, and `string` receives a string freed below.
    unsafe { ConvertSidToStringSidW(sid, &mut string) }.map_err(error)?;
    // SAFETY: `ConvertSidToStringSidW` wrote a NUL-terminated string.
    let user = String::from_utf16_lossy(unsafe { string.as_wide() });
    // SAFETY: allocated with `LocalAlloc`, and not used after this.
    unsafe { LocalFree(Some(HLOCAL(string.0.cast()))) };

    // A protected list, so nothing is inherited into it: the user alone,
    // and what is made inside inherits it.
    let sddl = HSTRING::from(format!("D:P(A;OICI;FA;;;{user})"));
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `sddl` is a valid string, and `descriptor` receives memory
    // freed below.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &sddl,
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
    }
    .map_err(error)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: `wide` is NUL-terminated and alive, and `attributes` points to
    // a descriptor alive until after the call. It fails if `path` exists.
    let created = unsafe { CreateDirectoryW(PCWSTR(wide.as_ptr()), Some(&attributes)) };
    // SAFETY: allocated with `LocalAlloc`; the folder keeps its own copy.
    unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    created.map_err(error)
}

#[cfg(test)]
mod tests;
