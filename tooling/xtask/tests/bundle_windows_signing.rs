#![cfg(feature = "dev")]
//! `bundle windows` with `[release.windows.signing]`, on any system, with
//! the commands recorded instead of run: the `.pfx` goes into a private
//! folder of the temporary folder for the run only, and is gone before
//! publishing and on every other path; `signtool` signs the app with it
//! before the installer and the zip are made; Inno Setup signs the installer
//! through xtask's sign helper; every signature is read back and judged by
//! its status. The password reaches only `signtool`, the PowerShell that
//! reads the `.pfx`, and ISCC's environment for the helper.

#[allow(dead_code)]
mod support;

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use support::{FakeRunner, Recorded, THUMBPRINT, TIMESTAMP, base64, tempdir, under_test};
use xtask::{
    Failure, Interrupt, WindowsArgs, WindowsBundled, WindowsHost, WindowsSecrets, bundle_windows,
    sign_windows_file,
};

const CERTIFICATE: &[u8] = b"the pfx hunter2";
const PASSWORD: &str = "password hunter2";

fn secrets() -> WindowsSecrets {
    WindowsSecrets {
        certificate: Some(base64(CERTIFICATE)),
        certificate_password: Some(PASSWORD.into()),
    }
}

fn options(config: &Path, out: &Path) -> WindowsArgs {
    WindowsArgs {
        config: Some(config.to_owned()),
        out: Some(out.to_owned()),
        ..WindowsArgs::default()
    }
}

/// A Windows host with fake `ISCC.exe` and `signtool.exe` on `PATH`, fake
/// `tar.exe`, `certutil.exe`, and Windows PowerShell in `%SystemRoot%`, and
/// a fake xtask.
fn host(temp: &Path) -> WindowsHost {
    let tools = temp.join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    for tool in ["ISCC.exe", "signtool.exe"] {
        std::fs::write(tools.join(tool), b"").unwrap();
    }
    let root = temp.join("Windows");
    let powershell = root.join("System32/WindowsPowerShell/v1.0");
    std::fs::create_dir_all(&powershell).unwrap();
    std::fs::write(powershell.join("powershell.exe"), b"").unwrap();
    for tool in ["tar.exe", "certutil.exe"] {
        std::fs::write(root.join("System32").join(tool), b"").unwrap();
    }
    WindowsHost {
        system: "windows".into(),
        path: Some(tools.into_os_string()),
        program_files_x86: None,
        system_root: Some(root.into_os_string()),
        xtask: Some(temp.join("xtask.exe")),
    }
}

fn powershell(temp: &Path) -> PathBuf {
    temp.join("Windows/System32/WindowsPowerShell/v1.0/powershell.exe")
}

fn bundle(
    options: &WindowsArgs,
    temp: &Path,
    runner: &mut FakeRunner,
) -> Result<WindowsBundled, Failure> {
    bundle_windows(
        options,
        &host(temp),
        &secrets(),
        &Interrupt::default(),
        runner,
    )
}

/// What a command is, by its program and, for PowerShell, its script.
fn kind(command: &Recorded) -> &'static str {
    let script = command.args.last().map(String::as_str).unwrap_or_default();
    match command.program.as_str() {
        "powershell" if script.contains("X509Certificate2Collection") => "read",
        "powershell" if script.contains("Get-AuthenticodeSignature") => "check",
        "cargo" => "cargo",
        "signtool" => "signtool",
        "ISCC" => "ISCC",
        "certutil" => "certutil",
        "tar" => "tar",
        other => panic!("unexpected {other}"),
    }
}

fn kinds(runner: &FakeRunner) -> Vec<&'static str> {
    runner.commands.iter().map(kind).collect()
}

fn env<'a>(command: &'a Recorded, key: &str) -> Option<Option<&'a str>> {
    command
        .envs
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_deref())
}

/// The order of a signed run that succeeds.
const SIGNED: [&str; 10] = [
    "cargo", "cargo", "read", "signtool", "check", "ISCC", "check", "certutil", "tar", "certutil",
];

/// Keeps the path of the `.pfx` that PowerShell is given.
fn watch_pfx(runner: &mut FakeRunner) -> Arc<Mutex<Option<PathBuf>>> {
    let pfx = Arc::new(Mutex::new(None));
    let seen = pfx.clone();
    runner.on_command = Some(Box::new(move |command| {
        if let Some(Some(file)) = env(command, "XTASK_SIGN_PFX")
            && command.program == "powershell"
        {
            *seen.lock().unwrap() = Some(PathBuf::from(file));
        }
    }));
    pfx
}

/// A signature report of `Get-AuthenticodeSignature`.
fn report(status: &str, kind: &str, signer: &str, timestamp: &str) -> String {
    format!("status={status}\r\ntype={kind}\r\nsigner={signer}\r\ntimestamp={timestamp}\r\n")
}

/// The end of the refusal of a signature that is not `Valid`.
const NOT_VALID: &str = "not Valid; a certificate that no trusted root vouches for, such as a self-signed one, must be trusted on this machine";

#[test]
fn the_app_then_the_installer_are_signed_with_the_pfx() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let mut runner = FakeRunner::new(temp.path());
    let iss = Arc::new(Mutex::new(String::new()));
    let pfx = Arc::new(Mutex::new(None::<PathBuf>));
    let (seen_iss, seen_pfx) = (iss.clone(), pfx.clone());
    runner.on_command = Some(Box::new(move |command| {
        if let Some(Some(file)) = env(command, "XTASK_SIGN_PFX")
            && command.program == "powershell"
        {
            *seen_pfx.lock().unwrap() = Some(PathBuf::from(file));
        }
        if command.program == "ISCC" {
            *seen_iss.lock().unwrap() =
                std::fs::read_to_string(command.args.last().unwrap()).unwrap();
        }
    }));
    let bundled = bundle(
        &options(&under_test("signed"), &out),
        temp.path(),
        &mut runner,
    )
    .unwrap();
    assert!(bundled.installer.0.is_file() && bundled.zip.0.is_file());
    assert_eq!(kinds(&runner), SIGNED);
    assert_eq!(
        runner.pfx_seen.as_deref(),
        Some(CERTIFICATE),
        "the .pfx is the decoded secret"
    );

    let pfx = pfx.lock().unwrap().clone().unwrap();
    let folder = pfx.parent().unwrap();
    assert_eq!(folder.parent(), Some(std::env::temp_dir().as_path()));
    assert!(
        folder
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("xtask-signing-"),
        "{}",
        folder.display()
    );
    assert!(!folder.exists(), "the .pfx's folder is gone");
    assert!(!pfx.starts_with(&out), "never under --out");

    let signtool = &runner.commands[3];
    let app = Path::new(signtool.args.last().unwrap());
    assert!(
        app.ends_with("payload/Signed Lab/Signed Lab.exe"),
        "{app:?}"
    );
    assert_eq!(signtool.path, temp.path().join("tools/signtool.exe"));
    assert_eq!(
        signtool.args,
        [
            "sign",
            "/fd",
            "sha256",
            "/tr",
            "http://timestamp.digicert.com",
            "/td",
            "sha256",
            "/f",
            pfx.to_str().unwrap(),
            "/p",
            PASSWORD,
            app.to_str().unwrap(),
        ]
    );
    assert_eq!(
        env(&runner.commands[4], "XTASK_SIGN_FILE"),
        Some(Some(app.to_str().unwrap())),
        "the app's signature is read back"
    );
    let staging = app.parent().unwrap().parent().unwrap().parent().unwrap();
    let setup = staging.join(format!(
        "Signed Lab-0.3.0-{}-setup.exe",
        std::env::consts::ARCH
    ));
    assert_eq!(
        env(&runner.commands[6], "XTASK_SIGN_FILE"),
        Some(Some(setup.to_str().unwrap())),
        "the installer's signature is read back"
    );

    let iscc = &runner.commands[5];
    let xtask = temp.path().join("xtask.exe");
    assert_eq!(iscc.args[0], "/Q");
    assert_eq!(
        iscc.args[1],
        format!("/Sxtask=$q{}$q sign-windows-file $f", xtask.display())
    );
    for arg in &iscc.args {
        assert!(!arg.contains(PASSWORD), "{arg}");
        assert!(!arg.contains(pfx.to_str().unwrap()), "{arg}");
    }
    let signtool_path = temp.path().join("tools/signtool.exe");
    for (key, value) in [
        ("XTASK_SIGN_SIGNTOOL", signtool_path.to_str().unwrap()),
        (
            "XTASK_SIGN_POWERSHELL",
            powershell(temp.path()).to_str().unwrap(),
        ),
        ("XTASK_SIGN_PFX", pfx.to_str().unwrap()),
        ("XTASK_SIGN_PASSWORD", PASSWORD),
        ("XTASK_SIGN_TIMESTAMP_URL", "http://timestamp.digicert.com"),
        ("XTASK_SIGN_THUMBPRINT", THUMBPRINT),
    ] {
        assert_eq!(env(iscc, key), Some(Some(value)), "{key} on ISCC");
    }
    let iss = iss.lock().unwrap().clone();
    assert!(iss.lines().any(|line| line == "SignTool=xtask"), "{iss}");
    assert!(!iss.contains(PASSWORD) && !iss.contains("certificate.pfx"));
}

#[test]
fn the_password_and_the_sign_context_reach_only_who_needs_them() {
    let temp = tempdir();
    let mut runner = FakeRunner::new(temp.path());
    bundle(
        &options(&under_test("signed"), &temp.path().join("out")),
        temp.path(),
        &mut runner,
    )
    .unwrap();
    let encoded = base64(CERTIFICATE);
    for command in &runner.commands {
        let kind = kind(command);
        for arg in &command.args {
            assert!(!arg.contains(&encoded), "{command:?}");
            if kind != "signtool" {
                assert!(!arg.contains("hunter2"), "{command:?}");
            }
        }
        // Each command gets back only the `XTASK_SIGN_*` it needs.
        let given: &[&str] = match kind {
            "read" => &["XTASK_SIGN_PFX", "XTASK_SIGN_PASSWORD", "XTASK_SIGN_USAGE"],
            "check" => &["XTASK_SIGN_FILE"],
            "ISCC" => &[
                "XTASK_SIGN_SIGNTOOL",
                "XTASK_SIGN_POWERSHELL",
                "XTASK_SIGN_PFX",
                "XTASK_SIGN_PASSWORD",
                "XTASK_SIGN_TIMESTAMP_URL",
                "XTASK_SIGN_THUMBPRINT",
            ],
            _ => &[],
        };
        for name in xtask::HELPER_VARIABLES {
            let value = env(command, name);
            if given.contains(&name) {
                assert!(
                    matches!(value, Some(Some(_))),
                    "{name} given to {command:?}"
                );
            } else {
                assert_eq!(value, Some(None), "{name} removed from {command:?}");
            }
        }
        if kind == "read" || kind == "ISCC" {
            assert_eq!(env(command, "XTASK_SIGN_PASSWORD"), Some(Some(PASSWORD)));
        }
        if command.program == "powershell" {
            assert_eq!(
                env(command, "PSModulePath"),
                Some(None),
                "Windows PowerShell builds its own module path: {command:?}"
            );
        }
        for name in ["WINDOWS_CERTIFICATE", "WINDOWS_CERTIFICATE_PASSWORD"] {
            assert_eq!(
                env(command, name),
                Some(None),
                "{name} removed from {command:?}"
            );
        }
    }
}

#[test]
fn the_xtask_path_is_escaped_for_inno_setup() {
    let temp = tempdir();
    let mut host = host(temp.path());
    let xtask = temp.path().join("a $b").join("xtask.exe");
    host.xtask = Some(xtask.clone());
    let mut runner = FakeRunner::new(temp.path());
    bundle_windows(
        &options(&under_test("signed"), &temp.path().join("out")),
        &host,
        &secrets(),
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap();
    assert_eq!(
        runner.commands[5].args[1],
        format!(
            "/Sxtask=$q{}$q sign-windows-file $f",
            xtask.display().to_string().replace('$', "$$")
        )
    );
}

#[test]
fn a_timestamp_url_with_a_double_quote_is_refused() {
    let temp = tempdir();
    let config = signed_with_url(temp.path(), "http://ts.example/a\"b");
    let mut runner = FakeRunner::new(temp.path());
    let failure = bundle(
        &options(&config, &temp.path().join("out")),
        temp.path(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "config");
    assert_eq!(
        failure.reason,
        format!(
            "{}: `release.windows.signing.timestamp_url` must not hold a double quote on Windows, where signtool's command line would read it as quoting",
            config.display()
        )
    );
    assert!(runner.commands.is_empty());
}

/// A copy of `signed` in `folder` with this timestamp server.
fn signed_with_url(folder: &Path, url: &str) -> PathBuf {
    let signed = under_test("signed");
    for file in ["icon.png", "entitlements.plist"] {
        std::fs::copy(signed.with_file_name(file), folder.join(file)).unwrap();
    }
    let text = std::fs::read_to_string(&signed).unwrap().replace(
        "timestamp_url = \"http://timestamp.digicert.com\"",
        &format!("timestamp_url = {url:?}"),
    );
    let path = folder.join("product.toml");
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn without_signing_nothing_changes() {
    let temp = tempdir();
    let mut runner = FakeRunner::new(temp.path());
    let staged = Arc::new(Mutex::new(String::new()));
    let seen = staged.clone();
    runner.on_command = Some(Box::new(move |command| {
        if command.program == "ISCC" {
            *seen.lock().unwrap() = std::fs::read_to_string(command.args.last().unwrap()).unwrap();
        }
    }));
    // The secrets are set, and still not used.
    bundle(
        &options(&under_test("plain"), &temp.path().join("out")),
        temp.path(),
        &mut runner,
    )
    .unwrap();
    assert_eq!(
        kinds(&runner),
        ["cargo", "cargo", "ISCC", "certutil", "tar", "certutil"]
    );
    assert_eq!(runner.commands[2].args.len(), 2, "no /S");
    assert_eq!(env(&runner.commands[2], "XTASK_SIGN_PASSWORD"), Some(None));
    assert!(!staged.lock().unwrap().contains("SignTool"));
    assert!(runner.pfx_seen.is_none());
}

#[test]
fn the_pfx_folder_is_gone_after_a_failure_at_each_step() {
    let bad = report("HashMismatch", "Authenticode", THUMBPRINT, TIMESTAMP);
    let good = report("Valid", "Authenticode", THUMBPRINT, TIMESTAMP);
    for (fail, containing, reports, step, last) in [
        (
            None,
            Some("X509Certificate2Collection"),
            vec![],
            "certificate",
            "read",
        ),
        (Some("signtool"), None, vec![], "sign-app", "signtool"),
        (None, None, vec![bad.clone()], "sign-app", "check"),
        (Some("ISCC"), None, vec![], "installer", "ISCC"),
        (
            None,
            None,
            vec![good.clone(), bad.clone()],
            "sign-installer",
            "check",
        ),
        (Some("certutil"), None, vec![], "installer", "certutil"),
        (Some("tar"), None, vec![], "zip", "tar"),
    ] {
        let temp = tempdir();
        let out = temp.path().join("out");
        let mut runner = FakeRunner::new(temp.path());
        let pfx = watch_pfx(&mut runner);
        runner.fail = fail;
        runner.fail_containing = containing;
        runner.signature_reports = reports.into();
        let failure = bundle(
            &options(&under_test("signed"), &out),
            temp.path(),
            &mut runner,
        )
        .unwrap_err();
        assert_eq!(failure.step, step, "{fail:?} {containing:?}");
        assert_eq!(kinds(&runner).last(), Some(&last), "{step}");
        assert!(!failure.reason.contains("hunter2"), "{}", failure.reason);
        let pfx = pfx.lock().unwrap().clone();
        let folder = pfx.as_deref().and_then(Path::parent).unwrap();
        assert!(!folder.exists(), "{step}: the .pfx's folder is gone");
        if step == "certificate" {
            assert!(
                !out.exists(),
                "the certificate is read before --out is made"
            );
        } else {
            assert_eq!(
                std::fs::read_dir(&out).unwrap().count(),
                0,
                "{step}: nothing published"
            );
        }
    }
}

#[test]
fn an_interrupt_stops_the_run_and_the_pfx_folder_is_gone() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let interrupt = Interrupt::default();
    let mut runner = FakeRunner::new(temp.path());
    let pfx = Arc::new(Mutex::new(None::<PathBuf>));
    let (signal, seen) = (interrupt.clone(), pfx.clone());
    runner.on_command = Some(Box::new(move |command| {
        if let Some(Some(file)) = env(command, "XTASK_SIGN_PFX") {
            *seen.lock().unwrap() = Some(PathBuf::from(file));
        }
        if command.program == "signtool" {
            signal.set();
        }
    }));
    let failure = bundle_windows(
        &options(&under_test("signed"), &out),
        &host(temp.path()),
        &secrets(),
        &interrupt,
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(
        (failure.step, failure.reason.as_str()),
        ("installer", "interrupted")
    );
    assert_eq!(
        kinds(&runner),
        ["cargo", "cargo", "read", "signtool", "check"]
    );
    let pfx = pfx.lock().unwrap().clone().unwrap();
    assert!(!pfx.parent().unwrap().exists());
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0);
}

#[test]
fn a_signature_passes_only_with_its_signer_a_timestamp_and_a_good_status() {
    let other = "1111111111111111111111111111111111111111";
    let accepted = [
        report("Valid", "Authenticode", THUMBPRINT, TIMESTAMP),
        report(
            "Valid",
            "Authenticode",
            &THUMBPRINT.to_lowercase(),
            TIMESTAMP,
        ),
    ];
    for accepted in accepted {
        let temp = tempdir();
        let mut runner = FakeRunner::new(temp.path());
        runner.signature_reports = [accepted.clone(), accepted.clone()].into();
        bundle(
            &options(&under_test("signed"), &temp.path().join("out")),
            temp.path(),
            &mut runner,
        )
        .unwrap_or_else(|failure| panic!("{accepted:?}: {failure}"));
    }
    for (rejected, reason) in [
        (
            report("HashMismatch", "Authenticode", THUMBPRINT, TIMESTAMP),
            format!("signature is HashMismatch, {NOT_VALID}"),
        ),
        (
            report("NotSigned", "None", "", ""),
            format!("is signed by nobody, not by the certificate {THUMBPRINT}"),
        ),
        (
            report("NotTrusted", "Authenticode", THUMBPRINT, TIMESTAMP),
            format!("signature is NotTrusted, {NOT_VALID}"),
        ),
        (
            // A test certificate that this machine does not trust.
            report("UnknownError", "Authenticode", THUMBPRINT, TIMESTAMP),
            format!("signature is UnknownError, {NOT_VALID}"),
        ),
        (
            report("Valid", "Authenticode", other, TIMESTAMP),
            format!("is signed by {other}, not by the certificate {THUMBPRINT}"),
        ),
        (
            report("Valid", "Authenticode", THUMBPRINT, ""),
            "signature has no timestamp".to_owned(),
        ),
        (
            report("Valid", "Catalog", THUMBPRINT, TIMESTAMP),
            "has a signature of type \"Catalog\", not an Authenticode signature of its own"
                .to_owned(),
        ),
    ] {
        let temp = tempdir();
        let mut runner = FakeRunner::new(temp.path());
        runner.signature_reports = [rejected.clone()].into();
        let failure = bundle(
            &options(&under_test("signed"), &temp.path().join("out")),
            temp.path(),
            &mut runner,
        )
        .unwrap_err();
        assert_eq!(failure.step, "sign-app", "{rejected:?}");
        assert!(
            failure.reason.ends_with(&reason),
            "{rejected:?}: {}",
            failure.reason
        );
    }
}

#[test]
fn the_pfx_must_hold_exactly_one_code_signing_key() {
    for (printed, reason) in [
        (
            "count=0\r\n".to_owned(),
            "it holds 0 certificates with a private key and the code-signing usage, and signing needs exactly one",
        ),
        (
            "count=2\r\n".to_owned(),
            "it holds 2 certificates with a private key and the code-signing usage, and signing needs exactly one",
        ),
        (
            format!("count=1\r\nthumbprint={THUMBPRINT}\r\nextra\r\n"),
            "PowerShell printed something other than one thumbprint",
        ),
        (
            "count=1\r\nthumbprint=XYZ\r\n".to_owned(),
            "PowerShell printed no thumbprint",
        ),
        (
            "warning: something\r\n".to_owned(),
            "PowerShell printed no count of signing certificates",
        ),
    ] {
        let temp = tempdir();
        let mut runner = FakeRunner::new(temp.path());
        runner.leaf_report = Some(printed.clone());
        let failure = bundle(
            &options(&under_test("signed"), &temp.path().join("out")),
            temp.path(),
            &mut runner,
        )
        .unwrap_err();
        assert_eq!(failure.step, "certificate", "{printed:?}");
        assert_eq!(
            failure.reason,
            format!(
                "could not read WINDOWS_CERTIFICATE with WINDOWS_CERTIFICATE_PASSWORD: {reason}"
            )
        );
        assert_eq!(kinds(&runner), ["cargo", "cargo", "read"]);
    }
}

#[test]
fn a_failed_signtool_gives_a_fixed_message() {
    let temp = tempdir();
    let mut runner = FakeRunner::new(temp.path());
    runner.fail = Some("signtool");
    runner.signtool_output = format!("SignTool Error: bad password {PASSWORD} given\r\n");
    let failure = bundle(
        &options(&under_test("signed"), &temp.path().join("out")),
        temp.path(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "sign-app");
    let app = runner.commands[3].args.last().unwrap().clone();
    assert_eq!(
        failure.reason,
        format!("signtool exited with exit code: 1 signing {app}; its output is above")
    );
}

#[test]
fn a_bad_certificate_or_password_fails_naming_no_secret() {
    let temp = tempdir();
    let mut runner = FakeRunner::new(temp.path());
    runner.fail_containing = Some("X509Certificate2Collection");
    let failure = bundle(
        &options(&under_test("signed"), &temp.path().join("out")),
        temp.path(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "certificate");
    assert_eq!(
        failure.reason,
        "could not read WINDOWS_CERTIFICATE with WINDOWS_CERTIFICATE_PASSWORD: System.Security.Cryptography.CryptographicException",
        "only the exception's type, never PowerShell's own error block"
    );
    assert!(!failure.is_refusal());
    assert!(!temp.path().join("out").exists(), "--out is not made");
}

/// The environment `bundle windows` gives ISCC for the sign helper.
fn helper_env(temp: &Path) -> HashMap<&'static str, OsString> {
    let pfx = temp.join("certificate.pfx");
    std::fs::write(&pfx, CERTIFICATE).unwrap();
    HashMap::from([
        (
            "XTASK_SIGN_SIGNTOOL",
            temp.join("tools/signtool.exe").into_os_string(),
        ),
        ("XTASK_SIGN_POWERSHELL", powershell(temp).into_os_string()),
        ("XTASK_SIGN_PFX", pfx.into_os_string()),
        ("XTASK_SIGN_PASSWORD", PASSWORD.into()),
        ("XTASK_SIGN_TIMESTAMP_URL", "http://ts.example/".into()),
        ("XTASK_SIGN_THUMBPRINT", THUMBPRINT.into()),
    ])
}

#[test]
fn the_sign_helper_signs_and_checks_with_what_iscc_was_given() {
    let temp = tempdir();
    let vars = helper_env(temp.path());
    let file = temp.path().join("setup.exe");
    let mut runner = FakeRunner::new(temp.path());
    sign_windows_file(&file, &|name| vars.get(name).cloned(), &mut runner).unwrap();
    assert_eq!(kinds(&runner), ["signtool", "check"]);
    assert_eq!(
        runner.commands[0].path,
        temp.path().join("tools/signtool.exe")
    );
    assert_eq!(
        runner.commands[0].args,
        [
            "sign",
            "/fd",
            "sha256",
            "/tr",
            "http://ts.example/",
            "/td",
            "sha256",
            "/f",
            temp.path().join("certificate.pfx").to_str().unwrap(),
            "/p",
            PASSWORD,
            file.to_str().unwrap(),
        ]
    );
    assert_eq!(runner.commands[1].path, powershell(temp.path()));
    assert_eq!(
        env(&runner.commands[1], "XTASK_SIGN_PASSWORD"),
        Some(None),
        "the check does not get the password"
    );

    let mut failing = FakeRunner::new(temp.path());
    failing.fail = Some("signtool");
    failing.signtool_output = format!("echo {PASSWORD}\r\n");
    let reason =
        sign_windows_file(&file, &|name| vars.get(name).cloned(), &mut failing).unwrap_err();
    assert_eq!(
        reason,
        format!(
            "signtool exited with exit code: 1 signing {}; its output is above",
            file.display()
        )
    );

    let mut rejected = FakeRunner::new(temp.path());
    rejected.signature_reports = [report(
        "HashMismatch",
        "Authenticode",
        THUMBPRINT,
        TIMESTAMP,
    )]
    .into();
    let reason =
        sign_windows_file(&file, &|name| vars.get(name).cloned(), &mut rejected).unwrap_err();
    assert!(
        reason.ends_with(&format!("signature is HashMismatch, {NOT_VALID}")),
        "{reason}"
    );
}

#[test]
fn the_sign_helper_runs_only_from_a_bundle() {
    let temp = tempdir();
    let mut vars = helper_env(temp.path());
    vars.remove("XTASK_SIGN_PFX");
    let mut runner = FakeRunner::new(temp.path());
    let reason = sign_windows_file(
        &temp.path().join("setup.exe"),
        &|name| vars.get(name).cloned(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(
        reason,
        "XTASK_SIGN_PFX is not set: sign-windows-file runs only from bundle windows"
    );
    assert!(runner.commands.is_empty());

    let mut vars = helper_env(temp.path());
    vars.insert("XTASK_SIGN_PASSWORD", OsString::new());
    let reason = sign_windows_file(
        &temp.path().join("setup.exe"),
        &|name| vars.get(name).cloned(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(
        reason,
        "XTASK_SIGN_PASSWORD is empty: sign-windows-file runs only from bundle windows"
    );
    assert!(runner.commands.is_empty());
}

#[test]
fn the_sign_helper_gives_its_children_only_what_each_needs() {
    let temp = tempdir();
    let vars = helper_env(temp.path());
    let mut runner = FakeRunner::new(temp.path());
    sign_windows_file(
        &temp.path().join("setup.exe"),
        &|name| vars.get(name).cloned(),
        &mut runner,
    )
    .unwrap();
    for command in &runner.commands {
        let given: &[&str] = if command.program == "powershell" {
            &["XTASK_SIGN_FILE"]
        } else {
            &[]
        };
        for name in xtask::HELPER_VARIABLES {
            if given.contains(&name) {
                assert!(matches!(env(command, name), Some(Some(_))), "{command:?}");
            } else {
                assert_eq!(
                    env(command, name),
                    Some(None),
                    "{name} removed from {command:?}"
                );
            }
        }
    }
}

#[test]
fn a_dev_bundle_is_signed_too() {
    let temp = tempdir();
    let config = signed_with_url(temp.path(), "http://timestamp.digicert.com");
    let mut runner = FakeRunner::new(temp.path());
    let options = WindowsArgs {
        dev: true,
        ..options(&config, &temp.path().join("out"))
    };
    bundle(&options, temp.path(), &mut runner).unwrap();
    assert_eq!(kinds(&runner), SIGNED);
}

#[test]
fn missing_or_unreadable_secrets_are_refused_before_building() {
    for (secrets, reason) in [
        (
            WindowsSecrets::default(),
            "[release.windows.signing] needs WINDOWS_CERTIFICATE, WINDOWS_CERTIFICATE_PASSWORD in the environment",
        ),
        (
            WindowsSecrets {
                certificate: None,
                ..secrets()
            },
            "[release.windows.signing] needs WINDOWS_CERTIFICATE in the environment",
        ),
        (
            WindowsSecrets {
                certificate_password: None,
                ..secrets()
            },
            "[release.windows.signing] needs WINDOWS_CERTIFICATE_PASSWORD in the environment",
        ),
        (
            WindowsSecrets {
                certificate: Some("not base64 hunter2!".into()),
                ..secrets()
            },
            "WINDOWS_CERTIFICATE is not valid base64",
        ),
        (
            WindowsSecrets {
                certificate_password: Some(String::new()),
                ..secrets()
            },
            "WINDOWS_CERTIFICATE_PASSWORD is empty",
        ),
    ] {
        let temp = tempdir();
        let out = temp.path().join("out");
        let mut runner = FakeRunner::new(temp.path());
        let failure = bundle_windows(
            &options(&under_test("signed"), &out),
            &host(temp.path()),
            &secrets,
            &Interrupt::default(),
            &mut runner,
        )
        .unwrap_err();
        assert_eq!((failure.step, failure.reason.as_str()), ("secrets", reason));
        assert!(failure.is_refusal());
        assert!(runner.commands.is_empty(), "nothing ran");
        assert!(!out.exists());
    }
}

/// `target`, relative to `base`, both as the system spells them
/// (canonical, so `\\?\` and short names compare); `None` on two volumes.
fn relative_to(base: &Path, target: &Path) -> Option<PathBuf> {
    let (base, target) = (base.canonicalize().unwrap(), target.canonicalize().unwrap());
    let (base, target): (Vec<_>, Vec<_>) =
        (base.components().collect(), target.components().collect());
    if base.first() != target.first() {
        return None;
    }
    let common = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    let mut relative: PathBuf = std::iter::repeat_n(Path::new(".."), base.len() - common).collect();
    for part in &target[common..] {
        relative.push(part);
    }
    Some(relative)
}

#[test]
fn signtool_on_a_relative_path_entry_is_made_absolute() {
    // Under Cargo's target folder, which is on the volume of the current
    // folder, so that a relative path reaches it.
    let temp = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("signtool-relative-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).unwrap();
    let mut host = host(&temp);
    let tools = temp.join("tools");
    let Some(relative) = relative_to(&std::env::current_dir().unwrap(), &tools) else {
        eprintln!("the target folder is on another volume: skipped");
        return;
    };
    assert!(
        relative.is_relative() && relative.join("signtool.exe").is_file(),
        "{relative:?}"
    );
    host.path = Some(relative.into_os_string());
    let mut runner = FakeRunner::new(&temp);
    let result = bundle_windows(
        &options(&under_test("signed"), &temp.join("out")),
        &host,
        &secrets(),
        &Interrupt::default(),
        &mut runner,
    );
    let signtool = runner.commands[3].path.clone();
    let same =
        std::fs::canonicalize(&signtool).ok() == tools.join("signtool.exe").canonicalize().ok();
    let helper =
        env(&runner.commands[5], "XTASK_SIGN_SIGNTOOL").map(|value| value.map(str::to_owned));
    let _ = std::fs::remove_dir_all(&temp);
    result.unwrap();
    assert!(signtool.is_absolute(), "{}", signtool.display());
    assert!(same, "{}", signtool.display());
    assert_eq!(
        helper,
        Some(Some(signtool.to_str().unwrap().to_owned())),
        "the sign helper runs the same signtool"
    );
}

#[test]
fn signtool_is_found_in_the_highest_windows_kit() {
    let temp = tempdir();
    let mut host = host(temp.path());
    std::fs::remove_file(temp.path().join("tools/signtool.exe")).unwrap();
    let kits = temp.path().join("Program Files (x86)/Windows Kits/10/bin");
    for version in [
        "10.0.19041.0",
        "10.0.22621.0",
        "10.0.9200.0",
        "not-a-version",
    ] {
        let x64 = kits.join(version).join("x64");
        std::fs::create_dir_all(&x64).unwrap();
        std::fs::write(x64.join("signtool.exe"), b"").unwrap();
    }
    host.program_files_x86 = Some(temp.path().join("Program Files (x86)").into_os_string());
    let mut runner = FakeRunner::new(temp.path());
    bundle_windows(
        &options(&under_test("signed"), &temp.path().join("out")),
        &host,
        &secrets(),
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap();
    assert_eq!(
        runner.commands[3].path,
        kits.join("10.0.22621.0/x64/signtool.exe")
    );
}

#[test]
fn no_signtool_or_no_xtask_is_refused_before_building() {
    let temp = tempdir();
    let (first, second) = (temp.path().join("first"), temp.path().join("second"));
    let mut no_signtool = host(&first);
    std::fs::remove_file(first.join("tools/signtool.exe")).unwrap();
    no_signtool.program_files_x86 = Some(temp.path().join("nowhere").into_os_string());
    let mut no_xtask = host(&second);
    no_xtask.xtask = None;
    for (host, reason) in [
        (
            no_signtool,
            "signtool.exe was not found: it is neither on PATH nor in %ProgramFiles(x86)%\\Windows Kits\\10\\bin\\<version>\\x64",
        ),
        (
            no_xtask,
            "xtask's own executable was not found, and Inno Setup runs it to sign",
        ),
    ] {
        let mut runner = FakeRunner::new(temp.path());
        let failure = bundle_windows(
            &options(&under_test("signed"), &temp.path().join("out")),
            &host,
            &secrets(),
            &Interrupt::default(),
            &mut runner,
        )
        .unwrap_err();
        assert_eq!((failure.step, failure.reason.as_str()), ("tools", reason));
        assert!(failure.is_refusal());
        assert!(runner.commands.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn a_pfx_folder_that_cannot_be_removed_blocks_publishing() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempdir();
    let out = temp.path().join("out");
    let mut runner = FakeRunner::new(temp.path());
    let pfx = Arc::new(Mutex::new(None::<PathBuf>));
    let seen = pfx.clone();
    // Once the zip is made, nothing inside the folder can be deleted.
    runner.on_command = Some(Box::new(move |command| {
        if let Some(Some(file)) = env(command, "XTASK_SIGN_PFX")
            && command.program == "powershell"
        {
            *seen.lock().unwrap() = Some(PathBuf::from(file));
        }
        if command.program == "tar" {
            let folder = seen.lock().unwrap().clone().unwrap();
            let folder = folder.parent().unwrap();
            std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o500)).unwrap();
        }
    }));
    let failure = bundle(
        &options(&under_test("signed"), &out),
        temp.path(),
        &mut runner,
    )
    .unwrap_err();
    let folder = pfx
        .lock()
        .unwrap()
        .clone()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let kept = folder.exists();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::remove_dir_all(&folder).unwrap();
    assert_eq!(failure.step, "certificate");
    assert!(
        failure.reason.starts_with(&format!(
            "could not remove the certificate's folder {}: ",
            folder.display()
        )),
        "{}",
        failure.reason
    );
    assert!(
        failure.reason.ends_with("; delete it"),
        "{}",
        failure.reason
    );
    assert!(kept, "the folder could not be removed");
    assert_eq!(
        std::fs::read_dir(&out).unwrap().count(),
        0,
        "nothing is published while the .pfx is on disk"
    );
}

#[test]
fn a_signed_iscc_is_captured_and_an_unsigned_one_is_not() {
    let temp = tempdir();
    let mut runner = FakeRunner::new(temp.path());
    bundle(
        &options(&under_test("signed"), &temp.path().join("out")),
        temp.path(),
        &mut runner,
    )
    .unwrap();
    assert_eq!(
        runner.captured,
        ["powershell", "signtool", "ISCC"],
        "ISCC holds the password, so what it prints is shown hidden"
    );

    let mut failing = FakeRunner::new(temp.path());
    failing.fail = Some("ISCC");
    let failure = bundle(
        &options(&under_test("signed"), &temp.path().join("failed")),
        temp.path(),
        &mut failing,
    )
    .unwrap_err();
    assert_eq!(
        (failure.step, failure.reason.as_str()),
        (
            "installer",
            "ISCC exited with exit code: 1; Inno Setup printed why above"
        )
    );

    let mut unsigned = FakeRunner::new(temp.path());
    bundle(
        &options(&under_test("plain"), &temp.path().join("plain")),
        temp.path(),
        &mut unsigned,
    )
    .unwrap();
    assert!(unsigned.captured.is_empty(), "{:?}", unsigned.captured);
}
