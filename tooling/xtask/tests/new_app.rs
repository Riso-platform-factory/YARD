#![cfg(feature = "dev")]
//! `cargo xtask new-app` creates a developer's own application in
//! `application/` and adds it to the workspace's `Cargo.toml`. Every refusal
//! happens before writing anything, names the flag (or `application/`), and
//! leaves the workspace byte-identical.

#[allow(dead_code)]
mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use support::{TempDir, tempdir};
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
/// the real `app/yard/product.toml` and its icon) and two workspace crates
/// (`yard_ui` and `alpha`) to check glob expansion and package-name collisions.
fn workspace() -> TempDir {
    workspace_with_manifest(WORKSPACE_TOML)
}

fn workspace_with_manifest(manifest: &str) -> TempDir {
    let temp = tempdir();
    let root = temp.path();

    fs::write(root.join("Cargo.toml"), manifest).unwrap();

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

/// Verifies that the root `Cargo.toml` changed ONLY in `members` (gaining
/// `"application"`) and `default-members` (becoming `["application"]`), and
/// stayed byte-for-byte identical everywhere else.
fn assert_root_cargo_toml_updated(before: &str, after: &str) {
    let (before_members, before_rest) = extract_array_assignment(before, "members", "<MEMBERS>");
    let (_, before_rest) =
        extract_array_assignment(&before_rest, "default-members", "<DEFAULT_MEMBERS>");

    let (after_members, after_rest) = extract_array_assignment(after, "members", "<MEMBERS>");
    let (after_defaults, after_rest) =
        extract_array_assignment(&after_rest, "default-members", "<DEFAULT_MEMBERS>");

    assert_eq!(
        after_rest, before_rest,
        "root Cargo.toml changed outside `members` and `default-members`:\nbefore:\n{before}\nafter:\n{after}"
    );
    assert_eq!(
        after_members.len(),
        before_members.len() + 1,
        "`members` should gain exactly one entry (\"application\"): before={before_members:?}, after={after_members:?}"
    );
    for existing in &before_members {
        assert!(
            after_members.contains(existing),
            "`members` lost existing entry {existing:?}: {after_members:?}"
        );
    }
    assert!(
        after_members.contains(&"application".to_owned()),
        "`members` should include \"application\": {after_members:?}"
    );
    assert_eq!(
        after_defaults,
        vec!["application".to_owned()],
        "`default-members` should become [\"application\"]"
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

    let features = toml_section(text, "[features]");
    assert!(
        !features.iter().any(|line| line.starts_with("dev")),
        "generated application must have no `dev` feature:\n{text}"
    );

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
    for line in &deps {
        if line.starts_with("yard_") || line.starts_with("command_palette") {
            assert!(
                line.contains("default-features = false"),
                "every YARD crate dependency must set default-features = false: {line}"
            );
        }
    }

    let build_deps = toml_section(text, "[build-dependencies]");
    let build_config = build_deps
        .iter()
        .find(|line| line.starts_with("yard_build_config"))
        .unwrap_or_else(|| panic!("missing `yard_build_config` in [build-dependencies]:\n{text}"));
    assert!(
        build_config.contains("default-features = false"),
        "`yard_build_config` build-dependency must set default-features = false: {build_config}"
    );
}

#[test]
fn makes_the_application_and_updates_the_workspace() {
    let temp = workspace();
    let root = temp.path();

    let made = new_app(root, &north_star()).unwrap();
    assert_eq!(
        made,
        Made {
            folder: root.join("application"),
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

    let app = &made.folder;
    assert_eq!(
        relative_files(app),
        [
            "Cargo.toml",
            "README.md",
            "build.rs",
            "icon.png",
            "product.toml",
            "src/main.rs",
            "src/menus.rs",
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

    // application/Cargo.toml
    let app_cargo = fs::read_to_string(app.join("Cargo.toml")).unwrap();
    assert_application_cargo_toml(&app_cargo, "north-star");

    // application/build.rs
    let build_rs = fs::read_to_string(app.join("build.rs")).unwrap();
    assert!(
        build_rs.contains("yard_build_config::build_script::run()"),
        "build.rs must call yard_build_config::build_script::run():\n{build_rs}"
    );

    // application/src/main.rs
    let main_rs = fs::read_to_string(app.join("src/main.rs")).unwrap();
    assert!(
        main_rs.contains("yard_app::run"),
        "src/main.rs must call yard_app::run:\n{main_rs}"
    );
    assert!(
        main_rs.contains("yard_app::application!()"),
        "src/main.rs must build the application with yard_app::application!():\n{main_rs}"
    );
    assert!(
        main_rs.contains("command_palette"),
        "src/main.rs must include command_palette:\n{main_rs}"
    );
    assert!(
        main_rs.contains("menus"),
        "src/main.rs must pass menus:\n{main_rs}"
    );
    assert!(
        !main_rs.contains("feature = \"dev\"") && !main_rs.contains("yard::dev"),
        "src/main.rs must have no dev/review mode:\n{main_rs}"
    );

    // application/src/menus.rs
    let menus_rs = fs::read_to_string(app.join("src/menus.rs")).unwrap();
    assert!(
        menus_rs.contains("pub fn menus"),
        "src/menus.rs must define pub fn menus:\n{menus_rs}"
    );
    assert!(
        menus_rs.contains("product.name"),
        "src/menus.rs must use product.name:\n{menus_rs}"
    );

    // application/README.md
    let readme = fs::read_to_string(app.join("README.md")).unwrap();
    assert!(!readme.trim().is_empty(), "README.md must not be empty");
    assert!(
        readme.contains("cargo run") && readme.contains("bundle"),
        "README.md should document running and bundling:\n{readme}"
    );

    // Root Cargo.toml differs ONLY in members and default-members.
    let updated_root = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert_root_cargo_toml_updated(WORKSPACE_TOML, &updated_root);
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
            folder: root.join("application"),
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
    /// must name `application/product.toml`, never a temporary path.
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
            assert!(
                failure.reason.contains("application/product.toml"),
                "{}: config-reader refusal must name `application/product.toml`, got: {}",
                case.label,
                failure.reason
            );
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
                    "--icon: product config application/product.toml: `icon` {} is {dimensions}; it must be square and at least 512 × 512",
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
        failure.reason.contains("application/"),
        "expected reason to name `application/`, got: {}",
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

    let made = new_app(root, &north_star()).unwrap();
    assert_eq!(made.folder, root.join("application"));
    assert!(
        !staging.exists(),
        "leftover .application.new-app must be removed"
    );
    assert!(
        !made.folder.join("stale.txt").exists(),
        "stale files must not appear in application/"
    );
    assert!(made.folder.join("product.toml").is_file());
    let updated_root = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert_root_cargo_toml_updated(WORKSPACE_TOML, &updated_root);
}

#[test]
fn leftover_temp_manifest_is_removed_by_the_next_run() {
    let temp = workspace();
    let root = temp.path();
    let stale = root.join("Cargo.toml.new-app");
    fs::write(&stale, b"stale manifest from interrupted run").unwrap();

    let made = new_app(root, &north_star()).unwrap();
    assert_eq!(made.folder, root.join("application"));
    assert!(
        !stale.exists(),
        "leftover Cargo.toml.new-app must be removed"
    );
    assert!(made.folder.join("product.toml").is_file());
    let updated_root = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert_root_cargo_toml_updated(WORKSPACE_TOML, &updated_root);
}

#[test]
fn interrupted_application_without_workspace_entry_is_refused_with_recovery_instructions() {
    let temp = workspace();
    let root = temp.path();
    let app = root.join("application");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("partial.txt"), b"from interrupted run").unwrap();
    let before = snapshot(root);

    let failure = new_app(root, &north_star()).unwrap_err();
    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    let lower = failure.reason.to_ascii_lowercase();
    assert!(
        failure.reason.contains("application/")
            && lower.contains("delete")
            && lower.contains("again"),
        "expected reason to name `application/` and tell the developer to delete it and run again, got: {}",
        failure.reason
    );
    assert_eq!(
        snapshot(root),
        before,
        "refusal must leave workspace byte-identical"
    );
}

#[test]
fn multiline_members_keeps_layout_comments_and_trailing_comma() {
    let before = "\
[workspace]
resolver = \"3\"
members = [
    # Applications.
    \"app/*\", # sample app
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
    \"app/*\", # sample app
    # Core crates.
    \"crates/*\",
    \"application\",
]
default-members = [\"application\"]
";
    let temp = workspace_with_manifest(before);
    new_app(temp.path(), &north_star()).unwrap();
    let after = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
    assert_eq!(after, expected);
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
members = [\"app/*\", \"crates/*\", \"application\"]
default-members = [\"application\"]
exclude = [\"tooling/runner-checks\"]

[workspace.package]
edition = \"2024\"
";
    let temp = workspace_with_manifest(before);
    new_app(temp.path(), &north_star()).unwrap();
    let after = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
    assert_eq!(after, expected);
}

#[test]
fn existing_application_member_is_not_duplicated() {
    for existing in ["./application", "application", "application/"] {
        let before = format!(
            "\
[workspace]
resolver = \"3\"
members = [\"app/*\", \"crates/*\", \"{existing}\"]
default-members = [\"app/yard\"]
"
        );
        let expected = format!(
            "\
[workspace]
resolver = \"3\"
members = [\"app/*\", \"crates/*\", \"{existing}\"]
default-members = [\"application\"]
"
        );
        let temp = workspace_with_manifest(&before);
        new_app(temp.path(), &north_star()).unwrap();
        let after = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
        assert_eq!(
            after, expected,
            "members holding {existing:?} must not gain a second application entry"
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
members = [\"app/*\", \"crates/*\", \"application\"]
default-members = [\"application\"]

# Dependency comment that must stay byte for byte.
[workspace.dependencies]
yard_ui = { path = \"crates/yard_ui\", default-features = false, features = [\"dev\"] }
";
    let temp = workspace_with_manifest(before);
    new_app(temp.path(), &north_star()).unwrap();
    let after = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
    assert_eq!(after, expected);
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
            folder: root.join("application"),
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

    // Literal `yard` is not hardcoded when the sample has another name.
    let yard_args = NewAppArgs {
        name: "Yard!".into(),
        identifier: "com.acme.northstar".into(),
        scheme: Some("northstar".into()),
        ..NewAppArgs::default()
    };
    let made = new_app(root, &yard_args).unwrap();
    assert_eq!(made.package, "yard");
}
