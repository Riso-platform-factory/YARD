#![cfg(all(feature = "dev", windows))]
//! `cargo xtask bundle windows` for real: it builds yard with the `plain`
//! config, makes the installer with Inno Setup and the zip with Windows'
//! `tar.exe`, and the zip holds `Lab/Lab.exe`. It takes minutes and needs
//! Inno Setup, so it is ignored by default: `cargo test -p xtask --test
//! bundle_windows_end_to_end -- --ignored`.

#[allow(dead_code)]
mod support;

use std::process::Command;
use support::{tempdir, under_test};

/// Runs `command`, which `what` describes; a failure names `what` and
/// what it printed on stderr, never the command, whose environment may hold
/// a test password.
fn succeeds(what: &str, command: &mut Command) -> String {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{what} failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
#[ignore = "builds yard in release and runs Inno Setup"]
fn bundles_plain() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let stdout = succeeds(
        "bundle windows of plain",
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["bundle", "windows", "--config"])
            .arg(under_test("plain"))
            .arg("--out")
            .arg(&out)
            .current_dir(xtask::repository()),
    );
    let lines: Vec<&str> = stdout.lines().collect();
    for step in ["stages", "build", "icon", "bundle", "installer", "zip"] {
        assert!(
            lines.contains(&step),
            "the step {step} is printed: {stdout}"
        );
    }
    let arch = std::env::consts::ARCH;
    let setup = out.join(format!("Lab-0.1.0-{arch}-setup.exe"));
    let zip = out.join(format!("Lab-0.1.0-windows-{arch}.zip"));
    assert!(setup.is_file(), "{}", setup.display());
    assert!(zip.is_file(), "{}", zip.display());
    for path in [&setup, &zip] {
        let at = lines
            .iter()
            .position(|line| *line == path.to_str().unwrap())
            .unwrap_or_else(|| panic!("{} is printed: {stdout}", path.display()));
        assert!(
            at > 0 && lines[at - 1].starts_with("sha256 "),
            "its sha256 comes before it: {stdout}"
        );
    }
    let tar = std::path::Path::new(&std::env::var_os("SystemRoot").unwrap())
        .join("System32")
        .join("tar.exe");
    let listed = succeeds("tar -tf", Command::new(tar).arg("-tf").arg(&zip));
    assert!(
        listed.lines().any(|line| line == "Lab/Lab.exe"),
        "the zip holds Lab/Lab.exe: {listed}"
    );
}

/// Runs Windows PowerShell's `script`, its values from `envs`, and returns
/// what it printed, trimmed.
/// `what` describes it in a failure. Windows PowerShell builds its own
/// module path: one that PowerShell 7 set would hide its modules.
fn powershell(what: &str, script: &str, envs: &[(&str, &std::ffi::OsStr)]) -> String {
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(script.lines().collect::<Vec<_>>().join("; "))
        .env_remove("PSModulePath");
    for (key, value) in envs {
        command.env(key, value);
    }
    succeeds(what, &mut command).trim().to_owned()
}

/// The test certificate, trusted on this machine (`LocalMachine\Root` and
/// `TrustedPublisher`, which needs an administrator, as GitHub's runner is)
/// while this lives, so its signatures are `Valid`; removed when it drops,
/// even after a failure.
struct Trusted(Option<String>);

/// Removes the test certificate from both machine stores, then says
/// `removed` only once neither holds it.
const UNTRUST: &str = r"$ErrorActionPreference = 'Stop'
foreach ($store in 'Root', 'TrustedPublisher') { $path = 'Cert:\LocalMachine\' + $store + '\' + $env:XTASK_TEST_THUMBPRINT; if (Test-Path -Path $path) { Remove-Item -Path $path } }
foreach ($store in 'Root', 'TrustedPublisher') { if (Test-Path -Path ('Cert:\LocalMachine\' + $store + '\' + $env:XTASK_TEST_THUMBPRINT)) { throw ('the test certificate is still in LocalMachine\' + $store) } }
Write-Output 'removed'";

impl Trusted {
    fn new(certificate: &std::path::Path, thumbprint: &str) -> Self {
        let trusted = Self(Some(thumbprint.to_owned()));
        powershell(
            "trusting the test certificate",
            r"$ErrorActionPreference = 'Stop'
Import-Certificate -FilePath $env:XTASK_TEST_CER -CertStoreLocation Cert:\LocalMachine\Root | Out-Null
Import-Certificate -FilePath $env:XTASK_TEST_CER -CertStoreLocation Cert:\LocalMachine\TrustedPublisher | Out-Null",
            &[("XTASK_TEST_CER", certificate.as_os_str())],
        );
        trusted
    }

    /// Removes the trust, and fails the test if either store still holds
    /// the certificate.
    fn remove(mut self) {
        let thumbprint = self.0.take().unwrap();
        assert_eq!(
            powershell(
                "removing the trust of the test certificate",
                UNTRUST,
                &[("XTASK_TEST_THUMBPRINT", thumbprint.as_ref())],
            ),
            "removed"
        );
    }
}

impl Drop for Trusted {
    /// After a failure only, and never panics, as a failed assertion may be
    /// unwinding.
    fn drop(&mut self) {
        let Some(thumbprint) = self.0.take() else {
            return;
        };
        let removed = Command::new("powershell.exe")
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
            .arg(UNTRUST.lines().collect::<Vec<_>>().join("; "))
            .env_remove("PSModulePath")
            .env("XTASK_TEST_THUMBPRINT", &thumbprint)
            .output();
        if !removed.is_ok_and(|output| output.status.success()) {
            eprintln!(
                "could not remove the test certificate {thumbprint} from LocalMachine\\Root and TrustedPublisher"
            );
        }
    }
}

/// `path` as Inno Setup's command line takes it: without the `\\?\` that
/// a canonical Windows path starts with.
fn plain(path: &std::path::Path) -> String {
    let path = path.display().to_string();
    match path.strip_prefix(r"\\?\") {
        Some(rest) => rest.to_owned(),
        None => path,
    }
}

/// Runs an installer or uninstaller with `/LOG=<log>`; a failure prints the
/// log's last lines.
fn with_log(what: &str, command: &mut Command, log: &std::path::Path) {
    let output = command
        .arg(format!("/LOG={}", plain(log)))
        .output()
        .unwrap();
    if !output.status.success() {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        panic!(
            "{what} failed ({}); the end of its log:\n{}",
            output.status,
            lines[lines.len().saturating_sub(30)..].join("\n")
        );
    }
}

/// The thumbprint of the certificate that signed `file`.
fn signer(file: &std::path::Path) -> String {
    powershell(
        "reading a signer",
        "(Get-AuthenticodeSignature -LiteralPath $env:XTASK_TEST_FILE).SignerCertificate.Thumbprint",
        &[("XTASK_TEST_FILE", file.as_os_str())],
    )
}

/// Runs a bundle that is given the secrets; on a failure, says so without
/// printing the command, which holds them.
fn bundle_signed(out: &std::path::Path, pfx: &[u8], password: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["bundle", "windows", "--config"])
        .arg(under_test("signed"))
        .arg("--out")
        .arg(out)
        .env("WINDOWS_CERTIFICATE", support::base64(pfx))
        .env("WINDOWS_CERTIFICATE_PASSWORD", password)
        .current_dir(xtask::repository())
        .output()
        .unwrap()
}

#[test]
#[ignore = "builds yard in release, runs Inno Setup and signtool, and installs"]
fn bundles_signed_and_signs_every_file() {
    let temp = tempdir();
    let pfx = temp.path().join("test.pfx");
    let password = "yard test password";
    // A self-signed code-signing certificate, exported, then taken out of
    // the store: the run must sign from the .pfx alone.
    let cer = temp.path().join("test.cer");
    let thumbprint = powershell(
        "making the test certificate",
        r"$ErrorActionPreference = 'Stop'
$certificate = New-SelfSignedCertificate -Type CodeSigningCert -Subject 'CN=YARD Test Signing' -CertStoreLocation Cert:\CurrentUser\My
$password = ConvertTo-SecureString -String $env:XTASK_TEST_PASSWORD -AsPlainText -Force
Export-PfxCertificate -Cert $certificate -FilePath $env:XTASK_TEST_PFX -Password $password | Out-Null
Export-Certificate -Cert $certificate -FilePath $env:XTASK_TEST_CER | Out-Null
Remove-Item -Path ('Cert:\CurrentUser\My\' + $certificate.Thumbprint) -DeleteKey
Write-Output $certificate.Thumbprint",
        &[
            ("XTASK_TEST_PASSWORD", password.as_ref()),
            ("XTASK_TEST_PFX", pfx.as_os_str()),
            ("XTASK_TEST_CER", cer.as_os_str()),
        ],
    );
    assert_eq!(thumbprint.len(), 40, "{thumbprint}");
    let trusted = Trusted::new(&cer, &thumbprint);
    let in_store = || {
        powershell(
            "looking in CurrentUser\\My",
            r"Test-Path -Path ('Cert:\CurrentUser\My\' + $env:XTASK_TEST_THUMBPRINT)",
            &[("XTASK_TEST_THUMBPRINT", thumbprint.as_ref())],
        )
    };
    assert_eq!(in_store(), "False");
    let pfx_bytes = std::fs::read(&pfx).unwrap();
    let signing_folders = || {
        std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("xtask-signing-")
            })
            .count()
    };
    let folders_before = signing_folders();

    // A wrong password fails at the certificate, and no output holds it.
    let wrong = "wrong password 7f3a";
    let failed = bundle_signed(&temp.path().join("wrong"), &pfx_bytes, wrong);
    let (stdout, stderr) = (
        String::from_utf8_lossy(&failed.stdout),
        String::from_utf8_lossy(&failed.stderr),
    );
    assert!(!failed.status.success(), "a wrong password fails");
    assert!(
        stderr.contains(
            "certificate: could not read WINDOWS_CERTIFICATE with WINDOWS_CERTIFICATE_PASSWORD"
        ),
        "{stderr}"
    );
    assert!(
        !stdout.contains(wrong) && !stderr.contains(wrong),
        "the password is not printed"
    );
    assert_eq!(signing_folders(), folders_before, "no .pfx is left behind");

    let out = temp.path().join("out");
    let bundled = bundle_signed(&out, &pfx_bytes, password);
    let (stdout, stderr) = (
        String::from_utf8_lossy(&bundled.stdout),
        String::from_utf8_lossy(&bundled.stderr),
    );
    assert!(bundled.status.success(), "the signed bundle: {stderr}");
    assert!(
        !stdout.contains(password) && !stderr.contains(password),
        "the password is not printed"
    );
    for step in [
        "certificate",
        "sign-app",
        "installer",
        "sign-installer",
        "zip",
    ] {
        assert!(
            stdout.lines().any(|line| line == step),
            "the step {step} is printed: {stdout}"
        );
    }
    assert_eq!(in_store(), "False", "the store was never used");
    assert_eq!(signing_folders(), folders_before, "no .pfx is left behind");

    let windows_powershell = std::path::Path::new(&std::env::var_os("SystemRoot").unwrap())
        .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let mut runner = xtask::SystemRunner {
        interrupt: xtask::Interrupt::default(),
    };
    let mut check = |file: &std::path::Path| {
        xtask::check_authenticode(&windows_powershell, file, &thumbprint, &mut runner)
    };

    let arch = std::env::consts::ARCH;
    let setup = out.join(format!("Signed Lab-0.3.0-{arch}-setup.exe"));
    let zip = out.join(format!("Signed Lab-0.3.0-windows-{arch}.zip"));
    assert_eq!(signer(&setup), thumbprint, "the installer");
    check(&setup).unwrap();

    let unpacked = temp.path().join("unpacked");
    std::fs::create_dir(&unpacked).unwrap();
    let tar = std::path::Path::new(&std::env::var_os("SystemRoot").unwrap())
        .join("System32")
        .join("tar.exe");
    succeeds(
        "unpacking the zip",
        Command::new(&tar)
            .arg("-xf")
            .arg(&zip)
            .arg("-C")
            .arg(&unpacked),
    );
    let app = unpacked.join("Signed Lab").join("Signed Lab.exe");
    assert_eq!(signer(&app), thumbprint, "the app in the zip");
    check(&app).unwrap();

    // A copy with one byte changed fails the same check.
    let tampered = temp.path().join("tampered.exe");
    let mut bytes = std::fs::read(&app).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0xFF;
    std::fs::write(&tampered, bytes).unwrap();
    let rejected = check(&tampered).unwrap_err();
    assert!(rejected.contains("HashMismatch"), "{rejected}");

    let installed = temp.path().join("installed");
    with_log(
        "installing silently",
        Command::new(&setup)
            .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"])
            .arg(format!("/DIR={}", plain(&installed))),
        &temp.path().join("install.log"),
    );
    let uninstaller = installed.join("unins000.exe");
    assert_eq!(signer(&installed.join("Signed Lab.exe")), thumbprint);
    assert_eq!(signer(&uninstaller), thumbprint, "the uninstaller");
    check(&uninstaller).unwrap();
    with_log(
        "uninstalling silently",
        Command::new(&uninstaller).args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"]),
        &temp.path().join("uninstall.log"),
    );
    // The uninstaller goes on from a copy of itself and returns at once.
    for _ in 0..60 {
        if !installed.join("Signed Lab.exe").exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    assert!(!installed.join("Signed Lab.exe").exists(), "uninstalled");
    trusted.remove();
}
