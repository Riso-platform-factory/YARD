#![cfg(feature = "dev")]
//! `cargo xtask new-app` creates a developer's own application in
//! `app/<name>/` (beside `app/yard/`), adds it to `members` only when not
//! already covered, and sets it as the workspace's `default-members`. Every
//! refusal happens before writing anything, names the flag (or the existing
//! folder / application), and leaves the workspace byte-identical.

#[allow(dead_code)]
mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;
use support::{TempDir, new_app_manifest, new_app_workspace_targets, tempdir};
use xtask::{Made, NewAppArgs, new_app};

const WORKSPACE_TOML: &str = "\
# Workspace root comment that must survive untouched.
[workspace]
resolver = \"3\"
members = [\"app/*\", \"crates/*\"]
default-members = [\"app/yard\"]
exclude = [\"tooling/runner-checks\"]

[workspace.package]
edition = \"2024\"
publish = false
license = \"GPL-3.0-or-later\"

# Dependency comment preserved byte for byte.
[workspace.dependencies]
yard_ui = { path = \"crates/yard_ui\", default-features = false }
";

/// Builds a small temporary workspace containing the sample app (copied from
/// the real `app/yard/product.toml` and its icon), two workspace crates
/// (`yard_ui` and `alpha`), and a matching `Cargo.lock` (listing `gpui`, `log`,
/// and the other stub dependencies) to check glob expansion and package-name
/// collisions.
fn workspace() -> TempDir {
    let temp = workspace_with_manifest(WORKSPACE_TOML);
    fs::write(temp.path().join("Cargo.lock"), workspace_lockfile()).unwrap();
    temp
}

fn workspace_lockfile() -> &'static [u8] {
    static LOCK: OnceLock<Vec<u8>> = OnceLock::new();
    LOCK.get_or_init(|| {
        let seed = workspace_with_manifest(WORKSPACE_TOML);
        support::update::cargo_generate_lockfile(seed.path(), true);
        fs::read(seed.path().join("Cargo.lock")).unwrap()
    })
}

fn workspace_with_manifest(manifest: &str) -> TempDir {
    let temp = tempdir();
    let root = temp.path();

    fs::write(root.join("Cargo.toml"), new_app_manifest(manifest)).unwrap();

    let yard = root.join("app/yard");
    fs::create_dir_all(&yard).unwrap();
    fs::write(
        yard.join("Cargo.toml"),
        "[package]\nname = \"yard\"\nversion = \"0.1.0\"\nedition.workspace = true\n",
    )
    .unwrap();
    let real_yard = xtask::repository().join("app/yard");
    fs::copy(real_yard.join("product.toml"), yard.join("product.toml")).unwrap();
    fs::copy(real_yard.join("icon.png"), yard.join("icon.png")).unwrap();

    let yard_ui = root.join("crates/yard_ui");
    fs::create_dir_all(&yard_ui).unwrap();
    fs::write(
        yard_ui.join("Cargo.toml"),
        "[package]\nname = \"yard_ui\"\nversion = \"0.1.0\"\nedition.workspace = true\n",
    )
    .unwrap();

    let alpha = root.join("crates/alpha");
    fs::create_dir_all(&alpha).unwrap();
    fs::write(
        alpha.join("Cargo.toml"),
        "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\nedition.workspace = true\n",
    )
    .unwrap();

    new_app_workspace_targets(root);
    temp
}

fn north_star() -> NewAppArgs {
    NewAppArgs {
        name: "North Star".into(),
        identifier: "com.acme.northstar".into(),
        scheme: None,
        icon: None,
        version: None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    Dir,
    File(Vec<u8>),
}

/// Captures every directory and file (including hidden files) under `root`
/// with its exact bytes, to prove a refusal changes nothing on disk.
fn snapshot(root: &Path) -> BTreeMap<String, Entry> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Entry>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .display()
                .to_string()
                .replace('\\', "/");
            if entry.file_type().unwrap().is_dir() {
                out.insert(relative, Entry::Dir);
                walk(root, &path, out);
            } else {
                out.insert(relative, Entry::File(fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// Every file under `folder`, relative to it, sorted, using `/` separators.
fn relative_files(folder: &Path) -> Vec<String> {
    snapshot(folder)
        .into_iter()
        .filter_map(|(path, entry)| match entry {
            Entry::File(_) => Some(path),
            Entry::Dir => None,
        })
        .collect()
}

fn write_png(path: &Path, width: u32, height: u32) {
    image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([((x + y) % 256) as u8, 120, 180])
    })
    .save(path)
    .unwrap();
}

/// CRC-32 used by PNG chunks (ISO-HDLC, polynomial `0xEDB88320`).
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

fn push_png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// A PNG with a valid 512 × 512 `IHDR` and valid chunk framing up to `IEND`,
/// whose `IDAT` payload is corrupt so a header-only check passes while a full
/// decode fails.
fn write_corrupt_idat_png(path: &Path, width: u32, height: u32) {
    let mut bytes = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    push_png_chunk(&mut bytes, b"IHDR", &ihdr);
    push_png_chunk(&mut bytes, b"IDAT", b"not-a-valid-zlib-stream");
    push_png_chunk(&mut bytes, b"IEND", b"");
    fs::write(path, bytes).unwrap();
}

/// Extracts `key = [...]` from a TOML document, returning the quoted string
/// items inside `[...]` and the document with that assignment replaced by a
/// fixed marker.
fn extract_array_assignment(text: &str, key: &str, marker: &str) -> (Vec<String>, String) {
    let needle = format!("{key} =");
    let start = text
        .match_indices(&needle)
        .find(|(idx, _)| *idx == 0 || text.as_bytes()[*idx - 1] == b'\n')
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| panic!("missing `{key}` assignment in:\n{text}"));
    let open = text[start..]
        .find('[')
        .map(|offset| start + offset)
        .unwrap_or_else(|| panic!("missing `[` for `{key}` in:\n{text}"));
    let close = text[open..]
        .find(']')
        .map(|offset| open + offset)
        .unwrap_or_else(|| panic!("missing `]` for `{key}` in:\n{text}"));

    let inside = &text[open + 1..close];
    let mut items = Vec::new();
    let mut rest = inside;
    while let Some(quote) = rest.find('"') {
        let after_open = &rest[quote + 1..];
        let end = after_open
            .find('"')
            .unwrap_or_else(|| panic!("unterminated string in `{key}` array: {inside}"));
        items.push(after_open[..end].to_owned());
        rest = &after_open[end + 1..];
    }

    let mut replaced = String::with_capacity(text.len());
    replaced.push_str(&text[..start]);
    replaced.push_str(marker);
    replaced.push_str(&text[close + 1..]);
    (items, replaced)
}

/// Verifies that the root `Cargo.toml` changed ONLY in `default-members`
/// (becoming `[expected_member]`), leaving `members` and every other byte
/// identical.
fn assert_root_cargo_toml_defaults_updated(before: &str, after: &str, expected_member: &str) {
    let (_, before_rest) = extract_array_assignment(before, "default-members", "<DEFAULT_MEMBERS>");
    let (after_defaults, after_rest) =
        extract_array_assignment(after, "default-members", "<DEFAULT_MEMBERS>");

    assert_eq!(
        after_rest, before_rest,
        "root Cargo.toml changed outside `default-members`:\nbefore:\n{before}\nafter:\n{after}"
    );
    assert_eq!(
        after_defaults,
        vec![expected_member.to_owned()],
        "`default-members` should become [{expected_member:?}]"
    );
}

/// Returns the non-empty, non-comment lines inside a `[section]` or
/// `[[section]]` header of a TOML file.
fn toml_section<'a>(text: &'a str, header: &str) -> Vec<&'a str> {
    let mut lines = Vec::new();
    let mut active = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            active = line == header;
            continue;
        }
        if active {
            lines.push(line);
        }
    }
    lines
}

fn assert_application_cargo_toml(text: &str, expected_package: &str) {
    let manifest: toml::Table =
        toml::from_str(text).unwrap_or_else(|error| panic!("invalid Cargo.toml: {error}\n{text}"));

    let package = toml_section(text, "[package]");
    assert!(
        package
            .iter()
            .any(|line| *line == format!("name = \"{expected_package}\"")),
        "[package] should name {expected_package:?}:\n{text}"
    );
    assert!(
        package.contains(&"version = \"0.1.0\""),
        "[package] version must stay \"0.1.0\":\n{text}"
    );

    let bin = toml_section(text, "[[bin]]");
    assert!(
        bin.iter()
            .any(|line| *line == format!("name = \"{expected_package}\"")),
        "[[bin]] should name {expected_package:?}:\n{text}"
    );

    if let Some(features) = manifest.get("features").and_then(toml::Value::as_table) {
        assert!(
            !features.contains_key("dev"),
            "generated application must have no `dev` feature:\n{text}"
        );
        for (feature_name, enabled) in features {
            if let Some(enabled) = enabled.as_array() {
                assert!(
                    !enabled.iter().any(|item| {
                        item.as_str()
                            .is_some_and(|value| value == "dev" || value.ends_with("/dev"))
                    }),
                    "feature `{feature_name}` must not turn on a `dev` feature:\n{text}"
                );
            }
        }
    }

    let deps = toml_section(text, "[dependencies]");
    assert!(
        deps.iter().any(|line| {
            line.starts_with("gpui")
                && (line.contains("gpui.workspace = true") || line.contains("workspace = true"))
        }),
        "[dependencies] must depend on gpui with workspace = true:\n{text}"
    );
    for required in ["yard_app", "yard_product", "yard_shell", "command_palette"] {
        let line = deps
            .iter()
            .find(|line| line.starts_with(required))
            .unwrap_or_else(|| panic!("missing `{required}` in [dependencies]:\n{text}"));
        assert!(
            line.contains("default-features = false"),
            "`{required}` must have default-features = false: {line}"
        );
    }

    let dev_deps = manifest
        .get("dev-dependencies")
        .and_then(toml::Value::as_table)
        .unwrap_or_else(|| panic!("missing [dev-dependencies]:\n{text}"));
    let gpui_dev = dev_deps
        .get("gpui")
        .and_then(toml::Value::as_table)
        .unwrap_or_else(|| panic!("missing `gpui` in [dev-dependencies]:\n{text}"));
    assert_eq!(
        gpui_dev.get("workspace").and_then(toml::Value::as_bool),
        Some(true),
        "`gpui` in [dev-dependencies] must set workspace = true:\n{text}"
    );
    let gpui_features: Vec<&str> = gpui_dev
        .get("features")
        .and_then(toml::Value::as_array)
        .unwrap_or_else(|| panic!("`gpui` in [dev-dependencies] must set features:\n{text}"))
        .iter()
        .map(|value| value.as_str().expect("feature name must be a string"))
        .collect();
    assert!(
        gpui_features.contains(&"test-support"),
        "`gpui` in [dev-dependencies] must include `test-support` in features, got {gpui_features:?}:\n{text}"
    );
    let yard_test = dev_deps
        .get("yard_test")
        .and_then(toml::Value::as_table)
        .unwrap_or_else(|| panic!("missing `yard_test` in [dev-dependencies]:\n{text}"));
    assert_eq!(
        yard_test.get("workspace").and_then(toml::Value::as_bool),
        Some(true),
        "`yard_test` in [dev-dependencies] must set workspace = true:\n{text}"
    );
    assert_eq!(
        yard_test
            .get("default-features")
            .and_then(toml::Value::as_bool),
        Some(false),
        "`yard_test` in [dev-dependencies] must set default-features = false:\n{text}"
    );

    let build_deps = toml_section(text, "[build-dependencies]");
    let build_config = build_deps
        .iter()
        .find(|line| line.starts_with("yard_build_config"))
        .unwrap_or_else(|| panic!("missing `yard_build_config` in [build-dependencies]:\n{text}"));
    assert!(
        build_config.contains("default-features = false"),
        "`yard_build_config` build-dependency must set default-features = false: {build_config}"
    );

    let mut dep_tables: Vec<(&str, &toml::Table)> = Vec::new();
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = manifest.get(section).and_then(toml::Value::as_table) {
            dep_tables.push((section, table));
        }
    }
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for target in targets.values().filter_map(toml::Value::as_table) {
            for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
                if let Some(table) = target.get(section).and_then(toml::Value::as_table) {
                    dep_tables.push((section, table));
                }
            }
        }
    }
    for (section, table) in dep_tables {
        for (name, spec) in table {
            let spec_table = spec.as_table();
            if name.starts_with("yard_") || name == "command_palette" {
                assert_eq!(
                    spec_table
                        .and_then(|table| table.get("default-features"))
                        .and_then(toml::Value::as_bool),
                    Some(false),
                    "every YARD crate dependency in [{section}] must set default-features = false: `{name}`\n{text}"
                );
            }
            if let Some(features) = spec_table
                .and_then(|table| table.get("features"))
                .and_then(toml::Value::as_array)
            {
                assert!(
                    !features
                        .iter()
                        .any(|feature| feature.as_str() == Some("dev")),
                    "dependency `{name}` in [{section}] must not enable `dev`:\n{text}"
                );
            }
        }
    }
}

/// Returns the body of the closure passed to `capture_logs(...)` in Rust source.
fn capture_logs_body(text: &str) -> &str {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut call_open = None;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"//") {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if bytes[index] == b'"' {
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == b'"' {
                    index += 1;
                    break;
                } else {
                    index += 1;
                }
            }
            continue;
        }
        let prev_is_ident =
            index > 0 && (bytes[index - 1].is_ascii_alphanumeric() || bytes[index - 1] == b'_');
        if !prev_is_ident && text[index..].starts_with("capture_logs") {
            let mut after = index + "capture_logs".len();
            while after < bytes.len() && bytes[after].is_ascii_whitespace() {
                after += 1;
            }
            if after < bytes.len() && bytes[after] == b'(' {
                call_open = Some(after);
                break;
            }
        }
        index += 1;
    }
    let open =
        call_open.unwrap_or_else(|| panic!("expected a `capture_logs(...)` call in:\n{text}"));
    let mut depth = 1usize;
    let mut cursor = open + 1;
    let mut call_close = None;
    while cursor < bytes.len() {
        if bytes[cursor..].starts_with(b"//") {
            while cursor < bytes.len() && bytes[cursor] != b'\n' {
                cursor += 1;
            }
            continue;
        }
        if bytes[cursor] == b'"' {
            cursor += 1;
            while cursor < bytes.len() {
                if bytes[cursor] == b'\\' {
                    cursor = (cursor + 2).min(bytes.len());
                } else if bytes[cursor] == b'"' {
                    cursor += 1;
                    break;
                } else {
                    cursor += 1;
                }
            }
            continue;
        }
        match bytes[cursor] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    call_close = Some(cursor);
                    break;
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    let close =
        call_close.unwrap_or_else(|| panic!("unterminated `capture_logs(...)` call in:\n{text}"));
    let args = &text[open + 1..close];
    let first_bar = args
        .find('|')
        .unwrap_or_else(|| panic!("expected a closure inside `capture_logs(...)`:\n{text}"));
    let second_bar = args[first_bar + 1..]
        .find('|')
        .map(|offset| first_bar + 1 + offset)
        .unwrap_or_else(|| {
            panic!("unterminated closure parameters in `capture_logs(...)`:\n{text}")
        });
    &args[second_bar + 1..]
}

#[test]
fn makes_the_application_and_updates_the_workspace() {
    let temp = workspace();
    let root = temp.path();

    let made = new_app(root, &north_star()).unwrap();
    assert_eq!(
        made,
        Made {
            folder: root.join("app/north-star"),
            package: "north-star".to_owned(),
            placeholder_icon: true,
        }
    );

    assert!(
        !root.join(".application.new-app").exists(),
        "staging directory must not remain"
    );
    assert!(
        !root.join("Cargo.toml.new-app").exists(),
        "staging Cargo.toml must not remain"
    );
    for entry in fs::read_dir(root).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        assert!(
            !name.starts_with(".application.new-app"),
            "staging directory `{name}` must not remain at workspace root"
        );
    }
    let mut app_entries: Vec<String> = fs::read_dir(root.join("app"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    app_entries.sort();
    assert_eq!(
        app_entries,
        ["north-star", "yard"],
        "app/ must contain only the sample and the new application (no temporary folders)"
    );

    let app = &made.folder;
    assert!(
        app.join("extensions").is_dir(),
        "generated application must include an extensions/ folder"
    );
    let files: Vec<String> = relative_files(app)
        .into_iter()
        .filter(|path| !path.starts_with("extensions/"))
        .collect();
    assert_eq!(
        files,
        [
            "Cargo.toml",
            "README.md",
            "build.rs",
            "icon.png",
            "product.toml",
            "src/lib.rs",
            "src/main.rs",
            "src/menus.rs",
            "src/startup.rs",
            "tests/startup.rs",
        ]
    );

    // product.toml parses back through yard_build_config::read with every value.
    let product_toml = fs::read_to_string(app.join("product.toml")).unwrap();
    assert!(
        product_toml.contains("icon = \"icon.png\""),
        "product.toml should reference icon.png:\n{product_toml}"
    );
    let config = yard_build_config::read(&app.join("product.toml")).unwrap();
    assert_eq!(config.name, "North Star");
    assert_eq!(config.version, "0.1.0");
    assert_eq!(config.identifier, "com.acme.northstar");
    assert_eq!(config.url_scheme, "northstar");
    assert_eq!(config.icon, app.join("icon.png").canonicalize().unwrap());
    assert_eq!(config.update_url, None);

    // Placeholder icon is a valid square PNG >= 512 × 512 and not the sample's icon.
    let icon_bytes = fs::read(app.join("icon.png")).unwrap();
    let sample_icon_bytes = fs::read(root.join("app/yard/icon.png")).unwrap();
    assert_ne!(
        icon_bytes, sample_icon_bytes,
        "placeholder icon must not be the sample's icon"
    );
    let decoded = image::load_from_memory_with_format(&icon_bytes, image::ImageFormat::Png)
        .expect("placeholder icon must decode as PNG");
    assert_eq!(decoded.width(), decoded.height(), "icon must be square");
    assert!(decoded.width() >= 512, "icon must be at least 512 × 512");

    // app/north-star/Cargo.toml
    let app_cargo = fs::read_to_string(app.join("Cargo.toml")).unwrap();
    assert_application_cargo_toml(&app_cargo, "north-star");

    // app/north-star/build.rs
    let build_rs = fs::read_to_string(app.join("build.rs")).unwrap();
    assert!(
        build_rs.contains("yard_build_config::build_script::run()"),
        "build.rs must call yard_build_config::build_script::run():\n{build_rs}"
    );

    // app/north-star/src/lib.rs
    let lib_rs = fs::read_to_string(app.join("src/lib.rs")).unwrap();
    assert!(
        lib_rs.contains("pub mod menus;"),
        "src/lib.rs must declare `pub mod menus;`:\n{lib_rs}"
    );
    assert!(
        lib_rs.contains("pub mod startup;"),
        "src/lib.rs must declare `pub mod startup;`:\n{lib_rs}"
    );
    assert!(
        !lib_rs.contains("feature = \"dev\"") && !lib_rs.contains("mod dev"),
        "src/lib.rs must have no dev/review mode:\n{lib_rs}"
    );

    // app/north-star/src/startup.rs
    let startup_rs = fs::read_to_string(app.join("src/startup.rs")).unwrap();
    assert!(
        startup_rs.contains("pub const EXTENSIONS"),
        "src/startup.rs must define `pub const EXTENSIONS`:\n{startup_rs}"
    );
    assert!(
        startup_rs.contains("command_palette"),
        "src/startup.rs must include command_palette:\n{startup_rs}"
    );
    assert!(
        startup_rs.contains("pub fn application()"),
        "src/startup.rs must define `pub fn application()`:\n{startup_rs}"
    );
    assert!(
        startup_rs.contains("yard_app::application!()"),
        "src/startup.rs must build the application with yard_app::application!():\n{startup_rs}"
    );

    // app/north-star/src/main.rs
    let main_rs = fs::read_to_string(app.join("src/main.rs")).unwrap();
    let compact_main: String = main_rs
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    assert!(
        compact_main.contains(
            "yard_app::run(north_star::startup::application(),north_star::startup::EXTENSIONS,north_star::menus::menus)"
        ) || compact_main.contains(
            "yard_app::run(north_star::startup::application(),north_star::startup::EXTENSIONS,north_star::menus::menus,)"
        ),
        "src/main.rs must call yard_app::run with north_star::startup::application(), north_star::startup::EXTENSIONS, and north_star::menus::menus:\n{main_rs}"
    );
    assert!(
        !main_rs.contains("feature = \"dev\"") && !main_rs.contains("yard::dev"),
        "src/main.rs must have no dev/review mode:\n{main_rs}"
    );

    // app/north-star/src/menus.rs
    let menus_rs = fs::read_to_string(app.join("src/menus.rs")).unwrap();
    assert!(
        menus_rs.contains("pub fn menus"),
        "src/menus.rs must define pub fn menus:\n{menus_rs}"
    );
    assert!(
        menus_rs.contains("product.name"),
        "src/menus.rs must use product.name:\n{menus_rs}"
    );

    // app/north-star/tests/startup.rs
    let startup_test = fs::read_to_string(app.join("tests/startup.rs")).unwrap();
    assert!(
        startup_test.contains("yard_test::TestApp")
            || (startup_test.contains("yard_test::{") && startup_test.contains("TestApp")),
        "tests/startup.rs must use yard_test::TestApp:\n{startup_test}"
    );
    assert!(
        startup_test.contains(".application("),
        "tests/startup.rs must pass the application with `.application(`:\n{startup_test}"
    );
    assert!(
        startup_test.contains(".menus("),
        "tests/startup.rs must pass menus with `.menus(`:\n{startup_test}"
    );
    assert!(
        startup_test.contains("capture_logs"),
        "tests/startup.rs must use `capture_logs`:\n{startup_test}"
    );
    assert!(
        !startup_test.contains("capture_trace"),
        "tests/startup.rs must not use `capture_trace`:\n{startup_test}"
    );
    let capture_body = capture_logs_body(&startup_test);
    assert!(
        capture_body.contains("TestApp::builder()"),
        "tests/startup.rs must call `TestApp::builder()` inside the `capture_logs` closure:\n{startup_test}"
    );
    assert!(
        capture_body.contains(".boot("),
        "tests/startup.rs must call `.boot(` inside the `capture_logs` closure:\n{startup_test}"
    );
    assert!(
        startup_test.contains("assert_no_errors"),
        "tests/startup.rs must call `assert_no_errors`:\n{startup_test}"
    );
    assert_eq!(
        startup_test.matches("#[gpui::test]").count(),
        1,
        "tests/startup.rs must have one `#[gpui::test]`:\n{startup_test}"
    );
    assert!(
        startup_test.contains("north_star"),
        "tests/startup.rs must use the generated library crate `north_star`:\n{startup_test}"
    );

    // app/north-star/README.md
    let readme = fs::read_to_string(app.join("README.md")).unwrap();
    assert!(!readme.trim().is_empty(), "README.md must not be empty");
    assert!(
        readme.contains("cargo run") && readme.contains("bundle"),
        "README.md should document running and bundling:\n{readme}"
    );
    assert!(
        readme.contains("cargo test -p north-star"),
        "README.md should document testing with `cargo test -p north-star`:\n{readme}"
    );
    assert!(
        readme.contains("src/startup.rs"),
        "README.md should tell the developer to register extensions in `src/startup.rs`:\n{readme}"
    );

    // Root Cargo.toml differs ONLY in default-members (`app/*` already covers `app/north-star`).
    let updated_root = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert_root_cargo_toml_defaults_updated(
        &new_app_manifest(WORKSPACE_TOML),
        &updated_root,
        "app/north-star",
    );
}

#[test]
fn explicit_icon_scheme_and_version_are_used() {
    let temp = workspace();
    let root = temp.path();
    let fixtures = tempdir();
    let custom_icon = fixtures.path().join("brand.png");
    write_png(&custom_icon, 512, 512);
    let expected_icon_bytes = fs::read(&custom_icon).unwrap();

    let args = NewAppArgs {
        name: "North Star".into(),
        identifier: "com.acme.northstar".into(),
        scheme: Some("north-star-app".into()),
        icon: Some(custom_icon),
        version: Some("2.3.4".into()),
    };
    let made = new_app(root, &args).unwrap();
    assert_eq!(
        made,
        Made {
            folder: root.join("app/north-star"),
            package: "north-star".to_owned(),
            placeholder_icon: false,
        }
    );

    assert_eq!(
        fs::read(made.folder.join("icon.png")).unwrap(),
        expected_icon_bytes,
        "given icon must be copied byte for byte"
    );

    let config = yard_build_config::read(&made.folder.join("product.toml")).unwrap();
    assert_eq!(config.version, "2.3.4");
    assert_eq!(config.url_scheme, "north-star-app");

    // Cargo.toml keeps package version 0.1.0; product.toml holds the product version.
    let app_cargo = fs::read_to_string(made.folder.join("Cargo.toml")).unwrap();
    assert_application_cargo_toml(&app_cargo, "north-star");
}

const ALL_FLAGS: [&str; 5] = ["--name", "--identifier", "--scheme", "--version", "--icon"];

fn assert_no_temp_dir(label: &str, reason: &str) {
    let temp = std::env::temp_dir();
    let mut candidates = vec![temp.clone()];
    if let Ok(canonical) = temp.canonicalize()
        && canonical != temp
    {
        candidates.push(canonical);
    }
    for dir in candidates {
        let text = dir.display().to_string();
        let trimmed = text.trim_end_matches(['/', '\\']);
        assert!(
            !trimmed.is_empty() && !reason.contains(trimmed),
            "{label}: refusal reason must not contain temp dir {trimmed:?}, got: {reason}"
        );
    }
}

fn assert_flag(label: &str, reason: &str, expected: &str) {
    assert!(
        reason.contains(expected),
        "{label}: expected reason to name `{expected}`, got: {reason}"
    );
    if expected != "--name" {
        for other in ALL_FLAGS {
            if other != expected {
                assert!(
                    !reason.contains(other),
                    "{label}: expected only `{expected}` (not `{other}`), got: {reason}"
                );
            }
        }
    }
}

struct RefusalCase {
    label: &'static str,
    args: NewAppArgs,
    flag: &'static str,
    /// True when `yard_build_config::read` produces the refusal; its reason
    /// must name `app/<package>/product.toml` (or `product.toml` when the name
    /// yields no package), never a temporary path.
    from_config_reader: bool,
}

#[test]
fn refusals_name_their_flag_and_change_nothing() {
    let long_identifier = format!("com.acme.{}", "a".repeat(119));
    assert_eq!(long_identifier.len(), 128);
    let linux_desktop_overflow_identifier = format!(
        "com.acme.{}",
        "a".repeat(256 - ".desktop".len() - "com.acme.".len())
    );
    assert_eq!(
        linux_desktop_overflow_identifier.len() + ".desktop".len(),
        256
    );
    // Passes Windows x86_64 (255 UTF-16 units) but exceeds Windows aarch64 (256).
    let windows_aarch64_overflow_name = "a".repeat(255 - "-0.1.0-windows-x86_64.zip".len());
    // Passes macOS, Windows (both arches), and Linux x86_64 (255 bytes), but
    // exceeds Linux aarch64 (256 bytes).
    let linux_aarch64_only_overflow_name = "a".repeat(255 - "-0.1.0-linux-x86_64.tar.gz".len());

    let cases = vec![
        // Identifier refused by yard_build_config::read.
        RefusalCase {
            label: "single-label identifier",
            args: NewAppArgs {
                identifier: "northstar".into(),
                ..north_star()
            },
            flag: "--identifier",
            from_config_reader: true,
        },
        RefusalCase {
            label: "underscore in identifier",
            args: NewAppArgs {
                identifier: "com.acme.north_star".into(),
                ..north_star()
            },
            flag: "--identifier",
            from_config_reader: true,
        },
        RefusalCase {
            label: "empty identifier",
            args: NewAppArgs {
                identifier: String::new(),
                ..north_star()
            },
            flag: "--identifier",
            from_config_reader: true,
        },
        // Identifier refused by package_checks (Windows AppId 127-char limit).
        RefusalCase {
            label: "128-character identifier",
            args: NewAppArgs {
                identifier: long_identifier,
                ..north_star()
            },
            flag: "--identifier",
            from_config_reader: false,
        },
        // Identifier refused by Linux package_checks (<identifier>.desktop > 255 bytes).
        RefusalCase {
            label: "248-character identifier exceeding Linux .desktop limit",
            args: NewAppArgs {
                identifier: linux_desktop_overflow_identifier,
                ..north_star()
            },
            flag: "--identifier",
            from_config_reader: false,
        },
        // Scheme refused by yard_build_config::read.
        RefusalCase {
            label: "scheme starting with digit",
            args: NewAppArgs {
                scheme: Some("42star".into()),
                ..north_star()
            },
            flag: "--scheme",
            from_config_reader: true,
        },
        RefusalCase {
            label: "uppercase scheme",
            args: NewAppArgs {
                scheme: Some("NorthStar".into()),
                ..north_star()
            },
            flag: "--scheme",
            from_config_reader: true,
        },
        // Scheme cannot be derived from name.
        RefusalCase {
            label: "scheme cannot be derived from leading digits",
            args: NewAppArgs {
                name: "42 Notes".into(),
                scheme: None,
                ..north_star()
            },
            flag: "--scheme",
            from_config_reader: true,
        },
        // Version refused by yard_build_config::read.
        RefusalCase {
            label: "empty version",
            args: NewAppArgs {
                version: Some(String::new()),
                ..north_star()
            },
            flag: "--version",
            from_config_reader: true,
        },
        RefusalCase {
            label: "version with line break",
            args: NewAppArgs {
                version: Some("0.1.0\n".into()),
                ..north_star()
            },
            flag: "--version",
            from_config_reader: true,
        },
        // Version refused by yard_build_config::build_script::version_numbers (> 65535).
        RefusalCase {
            label: "version component over 65535",
            args: NewAppArgs {
                version: Some("1.65536.0".into()),
                ..north_star()
            },
            flag: "--version",
            from_config_reader: false,
        },
        // Version refused by package_checks.
        RefusalCase {
            label: "version with slash",
            args: NewAppArgs {
                version: Some("0.1/0".into()),
                ..north_star()
            },
            flag: "--version",
            from_config_reader: false,
        },
        RefusalCase {
            label: "version with Inno Setup braces",
            args: NewAppArgs {
                version: Some("0.1.0-{#x}".into()),
                ..north_star()
            },
            flag: "--version",
            from_config_reader: false,
        },
        // Name refused by yard_build_config::read.
        RefusalCase {
            label: "empty name",
            args: NewAppArgs {
                name: String::new(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: true,
        },
        RefusalCase {
            label: "name with no ASCII letters or digits",
            args: NewAppArgs {
                name: "!!!".into(),
                scheme: Some("bang".into()),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: true,
        },
        // Name refused by package_checks on at least one system/arch.
        RefusalCase {
            label: "name with slash",
            args: NewAppArgs {
                name: "North/Star".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "Windows device name CON",
            args: NewAppArgs {
                name: "CON".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "name with Inno Setup braces",
            args: NewAppArgs {
                name: "North {Star}".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "name starting with dash",
            args: NewAppArgs {
                name: "-North Star".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "name exceeding Windows aarch64 file name limit",
            args: NewAppArgs {
                name: windows_aarch64_overflow_name,
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "name exceeding Linux aarch64 archive name limit only",
            args: NewAppArgs {
                name: linux_aarch64_only_overflow_name,
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        // Names Cargo refuses for a package or binary name.
        RefusalCase {
            label: "Linux name starting with a digit",
            args: NewAppArgs {
                name: "42 Notes".into(),
                scheme: Some("notes".into()),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "Cargo reserved binary name build",
            args: NewAppArgs {
                name: "Build".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "Cargo reserved binary name deps",
            args: NewAppArgs {
                name: "Deps".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "Cargo reserved binary name examples",
            args: NewAppArgs {
                name: "Examples".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "Cargo reserved binary name incremental",
            args: NewAppArgs {
                name: "Incremental".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "Rust keyword fn",
            args: NewAppArgs {
                name: "Fn".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "Rust keyword self",
            args: NewAppArgs {
                name: "Self".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "Rust keyword crate",
            args: NewAppArgs {
                name: "Crate".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "Rust keyword match",
            args: NewAppArgs {
                name: "Match".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "Rust keyword async",
            args: NewAppArgs {
                name: "Async".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "Rust library crate name std",
            args: NewAppArgs {
                name: "std".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "Rust library crate name test",
            args: NewAppArgs {
                name: "Test".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        // Collisions with the sample (case-insensitive).
        RefusalCase {
            label: "sample name case-insensitive",
            args: NewAppArgs {
                name: "YARD".into(),
                scheme: Some("northstar".into()),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "sample identifier case-insensitive",
            args: NewAppArgs {
                identifier: "VN.RisoTech.Yard".into(),
                ..north_star()
            },
            flag: "--identifier",
            from_config_reader: false,
        },
        RefusalCase {
            label: "sample url_scheme",
            args: NewAppArgs {
                scheme: Some("yard".into()),
                ..north_star()
            },
            flag: "--scheme",
            from_config_reader: false,
        },
        RefusalCase {
            label: "derived scheme collides with sample url_scheme",
            args: NewAppArgs {
                name: "Y a r d".into(),
                scheme: None,
                ..north_star()
            },
            flag: "--scheme",
            from_config_reader: false,
        },
        // Name whose Linux name is `yard`.
        RefusalCase {
            label: "Linux name is yard",
            args: NewAppArgs {
                name: "Yard!".into(),
                scheme: Some("northstar".into()),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        // Linux name colliding with an existing workspace crate.
        RefusalCase {
            label: "exact workspace crate collision",
            args: NewAppArgs {
                name: "Alpha".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "workspace crate collision across dash and underscore",
            args: NewAppArgs {
                name: "Yard UI".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        // Collisions with package names already in the workspace's Cargo.lock.
        RefusalCase {
            label: "Cargo.lock package collision gpui",
            args: NewAppArgs {
                name: "GPUI".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "Cargo.lock package collision log",
            args: NewAppArgs {
                name: "Log".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
        RefusalCase {
            label: "Cargo.lock package collision across dash and underscore",
            args: NewAppArgs {
                name: "Yard Test".into(),
                ..north_star()
            },
            flag: "--name",
            from_config_reader: false,
        },
    ];

    for case in cases {
        let temp = workspace();
        let root = temp.path();
        let before = snapshot(root);

        let Err(failure) = new_app(root, &case.args) else {
            panic!("{}: expected refusal naming {}", case.label, case.flag);
        };
        assert_eq!(failure.step, "arguments", "{}: {failure}", case.label);
        assert!(failure.is_refusal(), "{}: {failure}", case.label);
        assert_flag(case.label, &failure.reason, case.flag);
        assert_no_temp_dir(case.label, &failure.reason);
        if case.from_config_reader {
            let package = yard_product::file_name(&case.args.name);
            if package.is_empty() {
                assert!(
                    failure.reason.contains("product.toml")
                        && !failure.reason.contains("application/")
                        && !failure.reason.contains("app//"),
                    "{}: config-reader refusal must name `product.toml` without a temp or malformed path, got: {}",
                    case.label,
                    failure.reason
                );
            } else {
                let expected = format!("app/{package}/product.toml");
                assert!(
                    failure.reason.contains(&expected),
                    "{}: config-reader refusal must name `{expected}`, got: {}",
                    case.label,
                    failure.reason
                );
            }
        }
        assert_eq!(
            snapshot(root),
            before,
            "{}: refusal must leave workspace byte-identical",
            case.label
        );
    }
}

#[test]
fn bad_icons_are_refused_and_change_nothing() {
    let fixtures = tempdir();

    let missing = fixtures.path().join("does-not-exist.png");

    let not_png = fixtures.path().join("not-a-png.png");
    fs::write(&not_png, b"definitely not a png").unwrap();

    let small = fixtures.path().join("small.png");
    write_png(&small, 300, 300);

    let non_square = fixtures.path().join("non-square.png");
    write_png(&non_square, 512, 600);

    let full = fixtures.path().join("full.png");
    write_png(&full, 512, 512);
    let truncated = fixtures.path().join("truncated.png");
    let full_bytes = fs::read(&full).unwrap();
    fs::write(&truncated, &full_bytes[..full_bytes.len().min(64)]).unwrap();

    let corrupt_idat = fixtures.path().join("corrupt-idat.png");
    write_corrupt_idat_png(&corrupt_idat, 512, 512);

    for (label, icon, expected_dimensions) in [
        ("missing icon", missing, None),
        ("not a PNG", not_png, None),
        ("300x300 too small", small, Some("300 × 300")),
        ("512x600 not square", non_square, Some("512 × 600")),
        ("truncated PNG", truncated, None),
        ("corrupt IDAT fails full decode", corrupt_idat, None),
    ] {
        let temp = workspace();
        let root = temp.path();
        let before = snapshot(root);

        let args = NewAppArgs {
            icon: Some(icon.clone()),
            ..north_star()
        };
        let Err(failure) = new_app(root, &args) else {
            panic!("{label}: expected refusal naming --icon");
        };
        assert_eq!(failure.step, "arguments", "{label}: {failure}");
        assert!(failure.is_refusal(), "{label}: {failure}");
        assert_flag(label, &failure.reason, "--icon");
        if let Some(dimensions) = expected_dimensions {
            assert_eq!(
                failure.reason,
                format!(
                    "--icon: product config app/north-star/product.toml: `icon` {} is {dimensions}; it must be square and at least 512 × 512",
                    icon.display()
                ),
                "{label}: unexpected refusal reason"
            );
        }
        let mut without_input_path = failure.reason.replace(&icon.display().to_string(), "");
        if let Ok(canonical) = icon.canonicalize() {
            without_input_path = without_input_path.replace(&canonical.display().to_string(), "");
        }
        assert_no_temp_dir(label, &without_input_path);
        assert!(
            !without_input_path.contains("/private"),
            "{label}: refusal reason must not contain `/private`, got: {}",
            failure.reason
        );
        assert_eq!(
            snapshot(root),
            before,
            "{label}: refusal must leave workspace byte-identical"
        );
    }
}

#[test]
fn existing_application_is_refused_and_changes_nothing() {
    let temp = workspace();
    let root = temp.path();
    new_app(root, &north_star()).unwrap();
    let before = snapshot(root);

    let failure = new_app(root, &north_star()).unwrap_err();
    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert!(
        failure.reason.contains("app/north-star"),
        "expected reason to name `app/north-star`, got: {}",
        failure.reason
    );
    assert_eq!(
        snapshot(root),
        before,
        "refusal must leave workspace byte-identical"
    );
}

#[test]
fn second_application_is_refused_and_changes_nothing() {
    // After creating a first application, a second `new-app` for another name is refused.
    let temp = workspace();
    let root = temp.path();
    new_app(root, &north_star()).unwrap();
    let before = snapshot(root);

    let second = NewAppArgs {
        name: "South Wind".into(),
        identifier: "com.acme.southwind".into(),
        ..NewAppArgs::default()
    };
    let failure = new_app(root, &second).unwrap_err();
    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert!(
        failure.reason.contains("app/north-star"),
        "expected reason to name existing application `app/north-star`, got: {}",
        failure.reason
    );
    assert_eq!(
        snapshot(root),
        before,
        "refusal must leave workspace byte-identical"
    );

    // Even when `app/yard` is also listed, any other entry in `default-members` refuses.
    let multi_default = "\
[workspace]
resolver = \"3\"
members = [\"app/*\", \"crates/*\"]
default-members = [\"app/yard\", \"app/other\"]
";
    let temp = workspace_with_manifest(multi_default);
    let root = temp.path();
    let before = snapshot(root);

    let failure = new_app(root, &north_star()).unwrap_err();
    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert!(
        failure.reason.contains("app/other"),
        "expected reason to name existing default member `app/other`, got: {}",
        failure.reason
    );
    assert_eq!(
        snapshot(root),
        before,
        "refusal must leave workspace byte-identical"
    );
}

#[test]
fn leftover_temp_folder_is_removed_by_the_next_run() {
    let temp = workspace();
    let root = temp.path();
    let staging = root.join(".application.new-app");
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("stale.txt"), b"leftover from interrupted run").unwrap();

    let prefixed_staging = root.join(".application.new-app-interrupted");
    fs::create_dir_all(&prefixed_staging).unwrap();
    fs::write(
        prefixed_staging.join("stale.txt"),
        b"leftover from interrupted run",
    )
    .unwrap();
    fs::write(
        prefixed_staging.join("Cargo.toml"),
        "[package]\nname = \"north-star\"\nversion = \"0.1.0\"\nedition.workspace = true\n",
    )
    .unwrap();

    // A refused run must leave the workspace (including leftover temp folders) untouched.
    let before_refusal = snapshot(root);
    let bad = NewAppArgs {
        identifier: "bad".into(),
        ..north_star()
    };
    let failure = new_app(root, &bad).unwrap_err();
    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert_eq!(
        snapshot(root),
        before_refusal,
        "refused run must not delete leftover temp folders"
    );

    // A run that goes ahead deletes leftover `.application.new-app*` folders at the root.
    let made = new_app(root, &north_star()).unwrap();
    assert_eq!(made.folder, root.join("app/north-star"));
    assert!(
        !staging.exists(),
        "leftover .application.new-app must be removed"
    );
    assert!(
        !prefixed_staging.exists(),
        "leftover .application.new-app-interrupted must be removed"
    );
    assert!(
        !made.folder.join("stale.txt").exists(),
        "stale files must not appear in app/north-star/"
    );
    let mut app_entries: Vec<String> = fs::read_dir(root.join("app"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    app_entries.sort();
    assert_eq!(
        app_entries,
        ["north-star", "yard"],
        "no temporary folder may appear inside app/"
    );
    assert!(made.folder.join("product.toml").is_file());
    let updated_root = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert_root_cargo_toml_defaults_updated(
        &new_app_manifest(WORKSPACE_TOML),
        &updated_root,
        "app/north-star",
    );
}

#[test]
fn leftover_temp_manifest_is_removed_by_the_next_run() {
    let temp = workspace();
    let root = temp.path();
    let stale = root.join("Cargo.toml.new-app");
    fs::write(&stale, b"stale manifest from interrupted run").unwrap();

    let made = new_app(root, &north_star()).unwrap();
    assert_eq!(made.folder, root.join("app/north-star"));
    assert!(
        !stale.exists(),
        "leftover Cargo.toml.new-app must be removed"
    );
    assert!(made.folder.join("product.toml").is_file());
    let updated_root = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert_root_cargo_toml_defaults_updated(
        &new_app_manifest(WORKSPACE_TOML),
        &updated_root,
        "app/north-star",
    );
}

#[test]
fn interrupted_application_without_default_member_is_refused_with_recovery_instructions() {
    // Interrupted after rename into `app/north-star/` (complete with Cargo.toml)
    // before `default-members` was updated from `["app/yard"]`.
    let temp = workspace();
    let root = temp.path();
    let app = root.join("app/north-star");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("partial.txt"), b"from interrupted run").unwrap();
    fs::write(
        app.join("Cargo.toml"),
        "[package]\nname = \"north-star\"\nversion = \"0.1.0\"\nedition.workspace = true\n",
    )
    .unwrap();
    let before = snapshot(root);

    let failure = new_app(root, &north_star()).unwrap_err();
    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    let lower = failure.reason.to_ascii_lowercase();
    assert!(
        failure.reason.contains("app/north-star/")
            && lower.contains("delete")
            && lower.contains("again"),
        "expected reason to name `app/north-star/` and tell the developer to delete it and run again, got: {}",
        failure.reason
    );
    assert_eq!(
        snapshot(root),
        before,
        "refusal must leave workspace byte-identical"
    );

    // Same interrupted state when `default-members` is absent from root Cargo.toml.
    let no_default_members = "\
[workspace]
resolver = \"3\"
members = [\"app/*\", \"crates/*\"]
";
    let temp = workspace_with_manifest(no_default_members);
    let root = temp.path();
    let app = root.join("app/north-star");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("partial.txt"), b"from interrupted run").unwrap();
    let before = snapshot(root);

    let failure = new_app(root, &north_star()).unwrap_err();
    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    let lower = failure.reason.to_ascii_lowercase();
    assert!(
        failure.reason.contains("app/north-star/")
            && lower.contains("delete")
            && lower.contains("again"),
        "expected reason to name `app/north-star/` and tell the developer to delete it and run again, got: {}",
        failure.reason
    );
    assert_eq!(
        snapshot(root),
        before,
        "refusal must leave workspace byte-identical"
    );
}

#[test]
fn members_without_app_glob_appends_application() {
    let before = "\
# Root comment preserved.
[workspace]
resolver = \"3\"
members = [\"app/yard\", \"crates/*\"]
default-members = [\"app/yard\"]
exclude = [\"tooling/runner-checks\"]
";
    let expected = "\
# Root comment preserved.
[workspace]
resolver = \"3\"
members = [\"app/yard\", \"crates/*\", \"app/north-star\"]
default-members = [\"app/north-star\"]
exclude = [\"tooling/runner-checks\"]
";
    let temp = workspace_with_manifest(before);
    let made = new_app(temp.path(), &north_star()).unwrap();
    assert_eq!(made.folder, temp.path().join("app/north-star"));
    let after = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
    assert_eq!(after, new_app_manifest(expected));
}

#[test]
fn multiline_members_keeps_layout_comments_and_trailing_comma() {
    let before = "\
[workspace]
resolver = \"3\"
members = [
    # Applications.
    \"app/yard\", # sample app
    # Core crates.
    \"crates/*\",
]
default-members = [\"app/yard\"]
";
    let expected = "\
[workspace]
resolver = \"3\"
members = [
    # Applications.
    \"app/yard\", # sample app
    # Core crates.
    \"crates/*\",
    \"app/north-star\",
]
default-members = [\"app/north-star\"]
";
    let temp = workspace_with_manifest(before);
    new_app(temp.path(), &north_star()).unwrap();
    let after = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
    assert_eq!(after, new_app_manifest(expected));
}

#[test]
fn missing_default_members_is_inserted_after_members() {
    let before = "\
[workspace]
resolver = \"3\"
members = [\"app/*\", \"crates/*\"]
exclude = [\"tooling/runner-checks\"]

[workspace.package]
edition = \"2024\"
";
    let expected = "\
[workspace]
resolver = \"3\"
members = [\"app/*\", \"crates/*\"]
default-members = [\"app/north-star\"]
exclude = [\"tooling/runner-checks\"]

[workspace.package]
edition = \"2024\"
";
    let temp = workspace_with_manifest(before);
    new_app(temp.path(), &north_star()).unwrap();
    let after = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
    assert_eq!(after, new_app_manifest(expected));
}

#[test]
fn covering_member_entry_leaves_members_unchanged() {
    for members_list in [
        "\"app/*\", \"crates/*\"",
        "\"./app/*\", \"crates/*\"",
        "\"app/yard\", \"crates/*\", \"app/north-star\"",
        "\"app/yard\", \"crates/*\", \"./app/north-star\"",
        "\"app/yard\", \"crates/*\", \"app/north-star/\"",
    ] {
        let before = format!(
            "\
[workspace]
resolver = \"3\"
members = [{members_list}]
default-members = [\"app/yard\"]
"
        );
        let expected = format!(
            "\
[workspace]
resolver = \"3\"
members = [{members_list}]
default-members = [\"app/north-star\"]
"
        );
        let temp = workspace_with_manifest(&before);
        new_app(temp.path(), &north_star()).unwrap();
        let after = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
        assert_eq!(
            after,
            new_app_manifest(&expected),
            "members [{members_list}] already covers app/north-star and must not change"
        );
    }
}

#[test]
fn comments_and_inline_table_elsewhere_stay_unchanged() {
    let before = "\
# Top-level workspace comment.
[workspace]
resolver = \"3\"
members = [\"app/*\", \"crates/*\"]
default-members = [\"app/yard\"]

# Dependency comment that must stay byte for byte.
[workspace.dependencies]
yard_ui = { path = \"crates/yard_ui\", default-features = false, features = [\"dev\"] }
";
    let expected = "\
# Top-level workspace comment.
[workspace]
resolver = \"3\"
members = [\"app/*\", \"crates/*\"]
default-members = [\"app/north-star\"]

# Dependency comment that must stay byte for byte.
[workspace.dependencies]
yard_ui = { path = \"crates/yard_ui\", default-features = false, features = [\"dev\"] }
";
    let temp = workspace_with_manifest(before);
    new_app(temp.path(), &north_star()).unwrap();
    let after = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
    assert_eq!(after, new_app_manifest(expected));
}

#[test]
fn exclude_directory_skips_crates_under_it() {
    let manifest = "\
[workspace]
resolver = \"3\"
members = [\"app/*\", \"crates/*\"]
default-members = [\"app/yard\"]
exclude = [\"crates\"]
";
    let temp = workspace_with_manifest(manifest);
    let root = temp.path();

    let args = NewAppArgs {
        name: "Alpha".into(),
        identifier: "com.acme.alpha".into(),
        ..NewAppArgs::default()
    };
    let made = new_app(root, &args).unwrap();
    assert_eq!(
        made,
        Made {
            folder: root.join("app/alpha"),
            package: "alpha".to_owned(),
            placeholder_icon: true,
        }
    );
}

#[test]
fn sample_linux_name_collision_comes_from_sample_config() {
    let temp = workspace();
    let root = temp.path();
    let yard = root.join("app/yard");
    fs::write(
        yard.join("Cargo.toml"),
        "[package]\nname = \"sample-app-crate\"\nversion = \"0.1.0\"\nedition.workspace = true\n",
    )
    .unwrap();
    fs::write(
        yard.join("product.toml"),
        "name = \"Sample App\"\nversion = \"0.1.0\"\nidentifier = \"com.example.sample\"\nurl_scheme = \"sample\"\nicon = \"icon.png\"\n",
    )
    .unwrap();
    support::update::cargo_generate_lockfile(root, true);

    let before = snapshot(root);
    let colliding = NewAppArgs {
        name: "Sample! App".into(),
        identifier: "com.acme.northstar".into(),
        scheme: Some("northstar".into()),
        ..NewAppArgs::default()
    };
    let failure = new_app(root, &colliding).unwrap_err();
    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert_flag("sample Linux name collision", &failure.reason, "--name");
    assert_no_temp_dir("sample Linux name collision", &failure.reason);
    assert_eq!(snapshot(root), before);

    // Literal `yard` is not hardcoded as the sample's Linux name: it is
    // refused because `app/yard/` already exists, not as a `--name` collision
    // with the sample.
    let yard_args = NewAppArgs {
        name: "Yard!".into(),
        identifier: "com.acme.northstar".into(),
        scheme: Some("northstar".into()),
        ..NewAppArgs::default()
    };
    let failure = new_app(root, &yard_args).unwrap_err();
    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert!(
        failure.reason.contains("app/yard") && !failure.reason.contains("--name"),
        "expected refusal naming `app/yard` (not `--name`), got: {}",
        failure.reason
    );
    assert_eq!(snapshot(root), before);
}

#[test]
fn adds_application_to_lock_without_moving_existing_packages() {
    use support::update::{cargo_generate_lockfile, lock_packages};

    let temp = workspace();
    let root = temp.path();
    let lock = root.join("Cargo.lock");
    cargo_generate_lockfile(root, true);
    let mut expected = lock_packages(&lock);

    new_app(root, &north_star()).unwrap();

    expected.push(("north-star".into(), "0.1.0".into(), None));
    expected.sort();
    let mut actual = lock_packages(&lock);
    actual.sort();
    assert_eq!(actual, expected);
}
