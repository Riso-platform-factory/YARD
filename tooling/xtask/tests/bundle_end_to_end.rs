#![cfg(all(feature = "dev", target_os = "macos"))]
//! `cargo xtask bundle macos` for real: it builds yard with the `spaced`
//! config, signs the bundle, and zips it; and with a copy of the `signed`
//! config, signs with a self-signed test identity from a temporary keychain
//! and makes a disk image. It takes minutes and needs macOS's tools, so it is
//! ignored by default: `cargo test -p xtask -- --ignored`.

#[allow(dead_code)]
mod support;

use std::path::Path;
use std::process::Command;
use support::{base64, signed_config, tempdir, under_test};

fn succeeds(command: &mut Command) -> String {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{command:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
#[ignore = "builds yard in release and runs macOS's tools"]
fn bundles_spaced_with_a_zip() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let config = under_test("spaced");
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["bundle", "macos", "--zip", "--config"])
        .arg(&config)
        .arg("--out")
        .arg(&out)
        .current_dir(xtask::repository())
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        output.status.success(),
        "bundling failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines: Vec<&str> = stdout.lines().collect();
    let app = out.join("Lab Notes Pro.app");
    assert_eq!(lines.last(), Some(&app.to_str().unwrap()), "{stdout}");
    for step in ["stages", "build", "icon", "bundle", "sign", "zip"] {
        assert!(
            lines.contains(&step),
            "the step {step} is printed: {stdout}"
        );
    }
    let digest = lines
        .iter()
        .find_map(|line| line.strip_prefix("sha256 "))
        .unwrap_or_else(|| panic!("no sha256 line: {stdout}"));

    let plist = app.join("Contents/Info.plist");
    succeeds(Command::new("plutil").arg("-lint").arg(&plist));
    let value = |key: &str| {
        succeeds(
            Command::new("plutil")
                .args(["-extract", key, "raw", "-o", "-"])
                .arg(&plist),
        )
        .trim()
        .to_owned()
    };
    assert_eq!(value("CFBundleName"), "Lab Notes Pro");
    assert_eq!(value("CFBundleIdentifier"), "test.yard.labnotes");
    assert_eq!(value("CFBundleShortVersionString"), "0.2.0");
    assert!(Path::new(&app.join("Contents/MacOS/Lab Notes Pro")).is_file());
    assert!(Path::new(&app.join("Contents/Resources/Lab Notes Pro.icns")).is_file());
    succeeds(
        Command::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&app),
    );

    let zip = out.join("Lab Notes Pro-0.2.0.zip");
    let listing = succeeds(Command::new("zipinfo").arg("-1").arg(&zip));
    assert!(
        listing.lines().any(|line| line == "Lab Notes Pro.app/"),
        "the zip holds the bundle: {listing}"
    );
    let sum = succeeds(Command::new("shasum").args(["-a", "256"]).arg(&zip));
    assert_eq!(
        sum.split_whitespace().next(),
        Some(digest),
        "the printed sha256"
    );
}

/// The user's keychain search list.
fn search_list() -> String {
    succeeds(Command::new("security").args(["list-keychains", "-d", "user"]))
}

/// Makes a self-signed code-signing identity, `Developer ID Application:
/// YARD Test (YARDTEST01)`, as a `.p12` with `password`.
fn test_identity(folder: &Path, password: &str) -> Vec<u8> {
    let settings = folder.join("identity.cnf");
    std::fs::write(
        &settings,
        "[req]\ndistinguished_name = dn\nprompt = no\n\
         [dn]\nCN = Developer ID Application: YARD Test (YARDTEST01)\nOU = YARDTEST01\n\
         [ext]\nbasicConstraints = critical,CA:false\nkeyUsage = critical,digitalSignature\n\
         extendedKeyUsage = critical,codeSigning\n",
    )
    .unwrap();
    succeeds(
        Command::new("openssl")
            .args([
                "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2",
            ])
            .arg("-keyout")
            .arg(folder.join("key.pem"))
            .arg("-out")
            .arg(folder.join("certificate.pem"))
            .arg("-config")
            .arg(&settings)
            .args(["-extensions", "ext"]),
    );
    // OpenSSL 3 and later encrypt a .p12 in a way `security import` does not
    // read without `-legacy`; LibreSSL has no such option.
    let version = succeeds(Command::new("openssl").arg("version"));
    let mut export = Command::new("openssl");
    export.args(["pkcs12", "-export"]);
    if !version.starts_with("LibreSSL") {
        export.arg("-legacy");
    }
    succeeds(
        export
            .arg("-inkey")
            .arg(folder.join("key.pem"))
            .arg("-in")
            .arg(folder.join("certificate.pem"))
            .arg("-out")
            .arg(folder.join("identity.p12"))
            .args(["-passout", &format!("pass:{password}")]),
    );
    std::fs::read(folder.join("identity.p12")).unwrap()
}

#[test]
#[ignore = "builds yard in release, runs macOS's tools, and adds a keychain to the search list for the run"]
fn signs_with_a_test_identity_and_makes_a_disk_image() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let config_folder = temp.path().join("config");
    let private = temp.path().join("tmp");
    std::fs::create_dir(&config_folder).unwrap();
    std::fs::create_dir(&private).unwrap();
    let config = signed_config(&config_folder, false, true);
    let password = "test identity password";
    let p12 = test_identity(temp.path(), password);
    let before = search_list();

    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["bundle", "macos", "--zip", "--config"])
        .arg(&config)
        .arg("--out")
        .arg(&out)
        .current_dir(xtask::repository())
        .env("MACOS_CERTIFICATE", base64(&p12))
        .env("MACOS_CERTIFICATE_PASSWORD", password)
        .env_remove("APPLE_NOTARIZATION_KEY")
        .env_remove("APPLE_NOTARIZATION_KEY_ID")
        .env_remove("APPLE_NOTARIZATION_ISSUER_ID")
        .env("TMPDIR", &private)
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(search_list(), before, "the search list is as before");
    assert_eq!(
        std::fs::read_dir(&private).unwrap().count(),
        0,
        "the temporary keychain's folder is gone"
    );
    assert!(
        output.status.success(),
        "bundling failed:\n{stdout}\n{stderr}"
    );
    assert!(!stderr.contains(password), "the password is not printed");
    let lines: Vec<&str> = stdout.lines().collect();
    let app = out.join("Signed Lab.app");
    assert_eq!(lines.last(), Some(&app.to_str().unwrap()), "{stdout}");
    for step in ["keychain", "sign", "zip", "dmg"] {
        assert!(
            lines.contains(&step),
            "the step {step} is printed: {stdout}"
        );
    }
    assert!(!lines.contains(&"notarize"), "notarize = false: {stdout}");

    let details = |path: &Path| {
        let output = Command::new("codesign")
            .args(["-dv", "--verbose=4"])
            .arg(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stderr).unwrap()
    };
    let authority = "Authority=Developer ID Application: YARD Test (YARDTEST01)";
    let app_details = details(&app);
    assert!(app_details.contains(authority), "{app_details}");
    assert!(
        app_details.contains("flags=0x10000(runtime)"),
        "{app_details}"
    );
    assert!(app_details.contains("Timestamp="), "{app_details}");
    let binary_details = details(&app.join("Contents/MacOS/Signed Lab"));
    assert!(binary_details.contains(authority), "{binary_details}");
    let entitlements = succeeds(
        Command::new("codesign")
            .args(["-d", "--entitlements", "-", "--xml"])
            .arg(&app),
    );
    assert!(
        entitlements.contains("com.apple.security.cs.allow-jit"),
        "{entitlements}"
    );

    let image = out.join("Signed Lab-0.3.0.dmg");
    let image_details = details(&image);
    assert!(image_details.contains(authority), "{image_details}");
    let digest = |prefix: &str| {
        lines
            .iter()
            .find_map(|line| line.strip_prefix(prefix))
            .unwrap_or_else(|| panic!("no {prefix}line: {stdout}"))
    };
    for (file, prefix) in [
        (out.join("Signed Lab-0.3.0.zip"), "sha256 "),
        (image.clone(), "dmg-sha256 "),
    ] {
        let sum = succeeds(Command::new("shasum").args(["-a", "256"]).arg(&file));
        assert_eq!(
            sum.split_whitespace().next(),
            Some(digest(prefix)),
            "{prefix}"
        );
    }
    let listing = succeeds(Command::new("hdiutil").args(["imageinfo"]).arg(&image));
    assert!(listing.contains("UDZO"), "{listing}");
}
