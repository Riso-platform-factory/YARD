//! Cargo runs `yard_build_config::build_script::run()` for real on a fixture
//! package in a temporary workspace, both with the package's own `product.toml`
//! and with `PRODUCT_CONFIG` pointing at another config.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

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
        "yard_build_config-cargo-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    TempDir(path)
}

fn crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn repository() -> PathBuf {
    crate_dir()
        .ancestors()
        .nth(2)
        .expect("the repository root exists")
        .to_path_buf()
}

fn under_test(name: &str) -> PathBuf {
    crate_dir()
        .join("tests/configs")
        .join(name)
        .join("product.toml")
}

/// The target directory this test runs in, so the fixture build reuses already
/// compiled dependencies.
fn target_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CARGO_TARGET_DIR")
        && !dir.is_empty()
    {
        let path = PathBuf::from(dir);
        return if path.is_absolute() {
            path
        } else {
            repository().join(path)
        };
    }
    let exe = std::env::current_exe().unwrap();
    let mut dir = exe.parent().unwrap();
    if dir.file_name().is_some_and(|name| name == "deps") {
        dir = dir.parent().unwrap();
    }
    if let Some(parent) = dir.parent() {
        dir = parent;
    }
    if !dir.join("CACHEDIR.TAG").is_file()
        && !dir.join(".rustc_info.json").is_file()
        && let Some(parent) = dir.parent()
        && (parent.join("CACHEDIR.TAG").is_file() || parent.join(".rustc_info.json").is_file())
    {
        dir = parent;
    }
    dir.to_path_buf()
}

fn toml_path(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

/// Writes a standalone fixture package whose `build.rs` calls
/// `yard_build_config::build_script::run()`, seeded with `tests/configs/other`
/// and the repository's `Cargo.lock`.
fn write_fixture(package_name: &str) -> TempDir {
    let folder = tempdir();
    let crate_path = toml_path(&crate_dir());
    fs::write(
        folder.path().join("Cargo.toml"),
        format!(
            r#"[workspace]

[package]
name = "{package_name}"
version = "0.1.0"
edition = "2024"

[build-dependencies]
yard_build_config = {{ path = "{crate_path}", default-features = false }}
"#
        ),
    )
    .unwrap();
    fs::copy(
        repository().join("Cargo.lock"),
        folder.path().join("Cargo.lock"),
    )
    .unwrap();
    fs::write(
        folder.path().join("build.rs"),
        "fn main() { yard_build_config::build_script::run() }\n",
    )
    .unwrap();

    let other_dir = under_test("other").parent().unwrap().to_path_buf();
    fs::copy(
        other_dir.join("product.toml"),
        folder.path().join("product.toml"),
    )
    .unwrap();
    fs::copy(other_dir.join("icon.png"), folder.path().join("icon.png")).unwrap();

    fs::create_dir_all(folder.path().join("src")).unwrap();
    fs::write(
        folder.path().join("src/main.rs"),
        r#"fn main() {
    println!("PRODUCT_NAME={}", env!("PRODUCT_NAME"));
    println!("PRODUCT_VERSION={}", env!("PRODUCT_VERSION"));
    println!("PRODUCT_IDENTIFIER={}", env!("PRODUCT_IDENTIFIER"));
    println!("PRODUCT_URL_SCHEME={}", env!("PRODUCT_URL_SCHEME"));
    println!("EXE={}", std::env::current_exe().unwrap().display());
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        let icon = include_bytes!(concat!(env!("OUT_DIR"), "/window_icon.png"));
        println!("WINDOW_ICON_LEN={}", icon.len());
    }
}
"#,
    )
    .unwrap();

    folder
}

/// Runs `cargo run --offline` in `folder` and returns the `KEY=VALUE` lines
/// printed by the fixture binary.
fn run_fixture(folder: &Path, product_config: Option<&Path>) -> BTreeMap<String, String> {
    let mut command = Command::new(env!("CARGO"));
    command
        .args(["run", "--offline"])
        .current_dir(folder)
        .env("CARGO_TARGET_DIR", target_dir());
    match product_config {
        Some(config) => {
            command.env("PRODUCT_CONFIG", config);
        }
        None => {
            command.env_remove("PRODUCT_CONFIG");
        }
    }
    let output = command.output().expect("cargo run executes");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "cargo run --offline failed:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    stdout
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((key.to_owned(), value.to_owned()))
        })
        .collect()
}

#[cfg(windows)]
fn utf16le(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

#[cfg(windows)]
fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn assert_printed_product(
    values: &BTreeMap<String, String>,
    name: &str,
    version: &str,
    identifier: &str,
    url_scheme: &str,
) {
    assert_eq!(
        values.get("PRODUCT_NAME").map(String::as_str),
        Some(name),
        "{values:?}"
    );
    assert_eq!(
        values.get("PRODUCT_VERSION").map(String::as_str),
        Some(version),
        "{values:?}"
    );
    assert_eq!(
        values.get("PRODUCT_IDENTIFIER").map(String::as_str),
        Some(identifier),
        "{values:?}"
    );
    assert_eq!(
        values.get("PRODUCT_URL_SCHEME").map(String::as_str),
        Some(url_scheme),
        "{values:?}"
    );

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        let length: usize = values
            .get("WINDOW_ICON_LEN")
            .expect("WINDOW_ICON_LEN printed on Linux/FreeBSD")
            .parse()
            .expect("WINDOW_ICON_LEN is a number");
        assert!(length > 0, "embedded window_icon.png must be non-empty");
    }

    #[cfg(windows)]
    {
        let exe = PathBuf::from(values.get("EXE").expect("EXE printed"));
        let bytes = fs::read(&exe).expect("built .exe can be read");
        assert!(
            contains_bytes(&bytes, &utf16le(name)),
            "built .exe {} contains UTF-16LE product name {name:?}",
            exe.display()
        );
        assert!(
            contains_bytes(&bytes, &utf16le("ProductVersion")),
            "built .exe {} contains UTF-16LE \"ProductVersion\"",
            exe.display()
        );
        assert!(
            contains_bytes(&bytes, &utf16le(version)),
            "built .exe {} contains UTF-16LE version {version:?}",
            exe.display()
        );
    }
}

#[test]
fn cargo_runs_the_build_script_with_the_package_s_product_toml() {
    let fixture = write_fixture("yard_build_script_fixture_default");
    let values = run_fixture(fixture.path(), None);
    assert_printed_product(
        &values,
        "Other Lab",
        "1.0.0",
        "test.yard.other",
        "yardother",
    );
}

#[test]
fn cargo_runs_the_build_script_with_product_config_override() {
    let fixture = write_fixture("yard_build_script_fixture_override");
    let plain = under_test("plain");
    let values = run_fixture(fixture.path(), Some(&plain));
    assert_printed_product(&values, "Lab", "0.1.0", "test.yard.lab", "yardlab");
}

#[test]
fn cargo_rebuilds_when_product_toml_or_product_config_changes() {
    let fixture = write_fixture("yard_build_script_fixture_rebuild");
    let initial = run_fixture(fixture.path(), None);
    assert_printed_product(
        &initial,
        "Other Lab",
        "1.0.0",
        "test.yard.other",
        "yardother",
    );

    let config_path = fixture.path().join("product.toml");
    let original = fs::read_to_string(&config_path).unwrap();
    let rewritten = original.replacen(r#"name = "Other Lab""#, r#"name = "Renamed Lab""#, 1);
    assert_ne!(rewritten, original, "fixture product.toml names Other Lab");
    fs::write(&config_path, rewritten).unwrap();

    let after_edit = run_fixture(fixture.path(), None);
    assert_printed_product(
        &after_edit,
        "Renamed Lab",
        "1.0.0",
        "test.yard.other",
        "yardother",
    );

    let plain = under_test("plain");
    let after_override = run_fixture(fixture.path(), Some(&plain));
    assert_printed_product(&after_override, "Lab", "0.1.0", "test.yard.lab", "yardlab");

    let after_unset = run_fixture(fixture.path(), None);
    assert_printed_product(
        &after_unset,
        "Renamed Lab",
        "1.0.0",
        "test.yard.other",
        "yardother",
    );
}
