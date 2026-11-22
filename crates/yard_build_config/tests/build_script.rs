//! The build script in `yard_build_config::build_script`: `cargo:` directives,
//! window icons for Linux and FreeBSD, Windows `.ico` and `.rc` files, and
//! version numbers.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use yard_build_config::build_script;

/// A folder of its own under the system's temporary folder, removed when it
/// drops.
struct TempDir(PathBuf);
impl TempDir {
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn tempdir() -> TempDir {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "yard_build_config-build-script-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    TempDir(path)
}

/// The folder of test configs in `tests/configs/`.
fn configs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/configs")
}

/// A config under test in `tests/configs/`.
fn under_test(name: &str) -> PathBuf {
    configs_dir().join(name).join("product.toml")
}

/// Sorted file names directly inside `folder`.
fn entry_names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn only_linux_and_freebsd_targets_need_the_window_icon() {
    assert!(build_script::needs_window_icon("linux"));
    assert!(build_script::needs_window_icon("freebsd"));
    for target_os in ["macos", "windows", "ios", "android"] {
        assert!(!build_script::needs_window_icon(target_os), "{target_os}");
    }
}

#[test]
fn an_icon_without_a_png_extension_is_decoded_as_a_png() {
    let folder = tempdir();
    let icon = folder.path().join("app-icon");
    fs::copy(under_test("plain").with_file_name("icon.png"), &icon).unwrap();
    let out = folder.path().join("window_icon.png");
    build_script::write_window_icon(&icon, &out).unwrap();
    let written = image::open(&out).expect("the window icon is a PNG");
    assert_eq!((written.width(), written.height()), (256, 256));
}

#[test]
fn the_windows_icon_holds_one_png_frame_for_each_size_windows_uses() {
    assert_eq!(build_script::WINDOWS_ICON_SIZES, [16, 24, 32, 48, 64, 256]);
    let folder = tempdir();
    let icon = folder.path().join("app.ico");
    build_script::write_windows_icon(&under_test("plain").with_file_name("icon.png"), &icon)
        .unwrap();
    let bytes = fs::read(&icon).unwrap();
    // ICONDIR: reserved 0, type 1 (icon), then the count of entries.
    assert_eq!(bytes[..4], [0, 0, 1, 0]);
    let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    let mut sizes = Vec::new();
    for entry in bytes[6..6 + 16 * count].chunks(16) {
        // A width or height of 0 means 256.
        let side = |byte: u8| if byte == 0 { 256 } else { u32::from(byte) };
        let length = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
        let offset = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;
        let frame = image::load_from_memory_with_format(
            &bytes[offset..offset + length],
            image::ImageFormat::Png,
        )
        .expect("each frame is a PNG");
        assert_eq!(
            (frame.width(), frame.height()),
            (side(entry[0]), side(entry[1]))
        );
        sizes.push(frame.width());
    }
    assert_eq!(sizes, build_script::WINDOWS_ICON_SIZES);
    // image's own ICO reader takes it too, and reads the largest frame.
    let largest = image::open(&icon).expect("the icon is an ICO");
    assert_eq!((largest.width(), largest.height()), (256, 256));
}

#[test]
fn an_icon_that_is_not_a_png_is_an_error_naming_the_file() {
    let folder = tempdir();
    let icon = folder.path().join("broken.png");
    fs::write(&icon, b"not a png").unwrap();
    let windows_error =
        build_script::write_windows_icon(&icon, &folder.path().join("app.ico")).unwrap_err();
    assert!(windows_error.contains("broken.png"), "{windows_error}");
    let window_error =
        build_script::write_window_icon(&icon, &folder.path().join("window_icon.png")).unwrap_err();
    assert!(window_error.contains("broken.png"), "{window_error}");
}

#[test]
fn the_windows_resource_names_icon_one_and_the_product_and_its_version() {
    let script = build_script::windows_resource(
        Path::new(r"C:\out\app.ico"),
        "Lab \"Notes\" Pro",
        "1.2.3-beta.4",
    )
    .unwrap();
    for line in [
        "#pragma code_page(65001)",
        r#"1 ICON "C:\\out\\app.ico""#,
        "1 VERSIONINFO",
        "FILEVERSION 1,2,3,0",
        "PRODUCTVERSION 1,2,3,0",
        r#"        VALUE "Translation", 0x409, 1200"#,
        r#"        BLOCK "040904B0""#,
        r#"            VALUE "FileDescription", "Lab ""Notes"" Pro""#,
        r#"            VALUE "FileVersion", "1.2.3-beta.4""#,
        r#"            VALUE "ProductName", "Lab ""Notes"" Pro""#,
        r#"            VALUE "ProductVersion", "1.2.3-beta.4""#,
    ] {
        assert!(
            script.lines().any(|got| got == line),
            "{line:?} in\n{script}"
        );
    }
    // The config has no company, so the resource names none.
    assert!(!script.contains("CompanyName"), "{script}");
}

#[test]
fn the_version_numbers_are_the_core_version_then_zero() {
    for (version, numbers) in [
        ("1.2.3", "1,2,3,0"),
        ("1.2.3+42", "1,2,3,0"),
        ("1.2.3-beta.4", "1,2,3,0"),
        ("1.2.3.4", "1,2,3,0"),
        ("7", "7,0,0,0"),
        ("1.2", "1,2,0,0"),
        ("1.2.65535", "1,2,65535,0"),
    ] {
        assert_eq!(
            build_script::version_numbers(version).unwrap(),
            numbers,
            "{version}"
        );
    }
}

#[test]
fn a_version_number_over_65535_is_an_error_saying_so() {
    for version in ["1.2.65536", "70000.0.0", "1.99999999999999999999.0"] {
        let error = build_script::version_numbers(version).unwrap_err();
        assert!(error.contains("65535"), "{version}: {error}");
        assert!(error.contains(version), "{version}: {error}");
        assert!(build_script::windows_resource(Path::new("app.ico"), "Lab", version).is_err());
    }
}

#[test]
fn directives_returns_the_cargo_lines_in_order_and_writes_only_the_target_s_files() {
    for (config_name, app, version, identifier, url_scheme) in [
        ("plain", "Lab", "0.1.0", "test.yard.lab", "yardlab"),
        (
            "spaced",
            "Lab Notes Pro",
            "0.2.0",
            "test.yard.labnotes",
            "yardlabnotes",
        ),
        (
            "other",
            "Other Lab",
            "1.0.0",
            "test.yard.other",
            "yardother",
        ),
    ] {
        let config_path = under_test(config_name);
        let icon = config_path
            .with_file_name("icon.png")
            .canonicalize()
            .unwrap();
        let expected_lines = vec![
            "cargo:rerun-if-env-changed=PRODUCT_CONFIG".to_owned(),
            format!("cargo:rerun-if-changed={}", config_path.display()),
            format!("cargo:rerun-if-changed={}", icon.display()),
            format!("cargo:rustc-env=PRODUCT_NAME={app}"),
            format!("cargo:rustc-env=PRODUCT_VERSION={version}"),
            format!("cargo:rustc-env=PRODUCT_IDENTIFIER={identifier}"),
            format!("cargo:rustc-env=PRODUCT_URL_SCHEME={url_scheme}"),
            format!("cargo:rustc-env=PRODUCT_ICON={}", icon.display()),
            "cargo:rustc-env=PRODUCT_UPDATE_URL=".to_owned(),
        ];

        for target_os in ["linux", "freebsd", "windows", "macos"] {
            let out_dir = tempdir();
            let lines = build_script::directives(&config_path, target_os, out_dir.path()).unwrap();
            assert_eq!(lines, expected_lines, "{config_name} on {target_os}");

            match target_os {
                "linux" | "freebsd" => {
                    assert_eq!(
                        entry_names(out_dir.path()),
                        ["window_icon.png"],
                        "{config_name} on {target_os}"
                    );
                    let written = image::open(out_dir.path().join("window_icon.png"))
                        .expect("window_icon.png is a PNG");
                    assert_eq!(
                        (written.width(), written.height()),
                        (256, 256),
                        "{config_name} on {target_os}"
                    );
                }
                "windows" => {
                    assert_eq!(
                        entry_names(out_dir.path()),
                        ["app.ico", "app.rc"],
                        "{config_name} on {target_os}"
                    );
                    let ico_path = out_dir.path().join("app.ico");
                    let ico = image::open(&ico_path).expect("app.ico is an ICO");
                    assert_eq!(
                        (ico.width(), ico.height()),
                        (256, 256),
                        "{config_name} on {target_os}"
                    );
                    let rc = fs::read_to_string(out_dir.path().join("app.rc")).unwrap();
                    let expected_rc =
                        build_script::windows_resource(&ico_path, app, version).unwrap();
                    assert_eq!(rc, expected_rc, "{config_name} on {target_os}");
                }
                "macos" => {
                    assert!(
                        entry_names(out_dir.path()).is_empty(),
                        "{config_name} on macos writes no files"
                    );
                }
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn directives_emits_product_update_url_when_configured() {
    let config_dir = tempdir();
    let plain_dir = under_test("plain").parent().unwrap().to_path_buf();
    fs::copy(
        plain_dir.join("icon.png"),
        config_dir.path().join("icon.png"),
    )
    .unwrap();
    let update_url = "https://releases.example.com/latest.json";
    let text = format!(
        "{}update_url = {update_url:?}\n",
        fs::read_to_string(plain_dir.join("product.toml")).unwrap()
    );
    let config_path = config_dir.path().join("product.toml");
    fs::write(&config_path, text).unwrap();

    let out_dir = tempdir();
    let lines = build_script::directives(&config_path, "macos", out_dir.path()).unwrap();
    assert_eq!(
        lines.last().map(String::as_str),
        Some(format!("cargo:rustc-env=PRODUCT_UPDATE_URL={update_url}").as_str())
    );
}

#[test]
fn directives_refuses_each_bad_config_naming_the_file_and_writes_nothing() {
    let mut bad_configs: Vec<PathBuf> = fs::read_dir(configs_dir())
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with("bad-")
                .then(|| entry.path().join("product.toml"))
        })
        .collect();
    bad_configs.sort();
    assert!(
        bad_configs.len() >= 9,
        "expected the bad-* test configs under tests/configs/: {bad_configs:?}"
    );

    for config_path in bad_configs {
        for target_os in ["linux", "freebsd", "windows", "macos"] {
            let out_dir = tempdir();
            let error =
                build_script::directives(&config_path, target_os, out_dir.path()).unwrap_err();
            assert!(
                error.contains(&config_path.display().to_string()),
                "{} on {target_os}: {error}",
                config_path.display()
            );
            assert!(
                entry_names(out_dir.path()).is_empty(),
                "{} on {target_os} must write nothing into out_dir",
                config_path.display()
            );
        }
    }
}

#[test]
fn directives_refuses_a_version_over_65535_naming_version_and_65535() {
    let config_dir = tempdir();
    let plain_dir = under_test("plain").parent().unwrap().to_path_buf();
    fs::copy(
        plain_dir.join("icon.png"),
        config_dir.path().join("icon.png"),
    )
    .unwrap();
    let text = fs::read_to_string(plain_dir.join("product.toml"))
        .unwrap()
        .replacen("version = \"0.1.0\"", "version = \"1.2.65536\"", 1);
    let config_path = config_dir.path().join("product.toml");
    fs::write(&config_path, text).unwrap();

    let out_dir = tempdir();
    let error = build_script::directives(&config_path, "windows", out_dir.path()).unwrap_err();
    assert!(error.contains("version"), "{error}");
    assert!(error.contains("65535"), "{error}");
    assert!(!out_dir.path().join("app.rc").exists());
}
