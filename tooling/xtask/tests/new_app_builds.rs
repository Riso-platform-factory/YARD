#![cfg(feature = "dev")]
//! End-to-end check that `new_app` generates an application that Cargo resolves
//! as the workspace's default member, compiles cleanly, and passes its starter
//! test inside a copy of the real repository.

#[allow(dead_code)]
mod support;

use std::fs;
use std::path::Path;
use std::process::Command;
use support::tempdir;
use xtask::{Made, NewAppArgs, new_app};

fn copy_workspace(source: &Path, destination: &Path, is_root: bool) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == "target" || name_str == ".git" || (is_root && name_str == "dist") {
            continue;
        }
        let file_type = entry.file_type().unwrap();
        if file_type.is_symlink() {
            continue;
        }
        let from = entry.path();
        let to = destination.join(&name);
        if file_type.is_dir() {
            copy_workspace(&from, &to, false);
        } else if file_type.is_file() {
            fs::copy(&from, &to).unwrap();
        }
    }
}

fn cargo(cwd: &Path) -> Command {
    let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.current_dir(cwd);
    if let Some(target_dir) = std::env::var_os("CARGO_TARGET_DIR").filter(|dir| !dir.is_empty()) {
        command.env("CARGO_TARGET_DIR", target_dir);
    }
    command
}

#[test]
#[ignore = "copies the workspace and builds; run by the lead"]
fn generated_application_checks_in_a_copy_of_the_workspace() {
    let temp = tempdir();
    let copy = temp.path();
    copy_workspace(&xtask::repository(), copy, true);

    let args = NewAppArgs {
        name: "North Star".into(),
        identifier: "com.acme.northstar".into(),
        ..NewAppArgs::default()
    };
    let made = new_app(copy, &args).unwrap();
    assert_eq!(
        made,
        Made {
            folder: copy.join("app/north-star"),
            package: "north-star".to_owned(),
            placeholder_icon: true,
        }
    );

    let metadata_output = cargo(copy)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .expect("cargo metadata runs");
    assert!(
        metadata_output.status.success(),
        "cargo metadata failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&metadata_output.stdout),
        String::from_utf8_lossy(&metadata_output.stderr),
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&metadata_output.stdout).expect("cargo metadata is JSON");
    let expected_manifest = copy
        .join("app/north-star/Cargo.toml")
        .canonicalize()
        .unwrap();
    let app_package = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| {
            package["name"] == "north-star"
                && package["manifest_path"]
                    .as_str()
                    .and_then(|path| Path::new(path).canonicalize().ok())
                    .as_deref()
                    == Some(expected_manifest.as_path())
        })
        .unwrap_or_else(|| {
            panic!(
                "expected package `north-star` at {} in cargo metadata",
                expected_manifest.display()
            )
        });
    let default_members = metadata["workspace_default_members"].as_array().unwrap();
    assert_eq!(
        default_members.as_slice(),
        std::slice::from_ref(&app_package["id"]),
        "expected `app/north-star` ({}) to be the sole entry of workspace_default_members: {default_members:?}",
        app_package["id"]
    );

    let check_output = cargo(copy)
        .args(["check", "-p", "north-star"])
        .output()
        .expect("cargo check runs");
    assert!(
        check_output.status.success(),
        "cargo check -p north-star failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&check_output.stdout),
        String::from_utf8_lossy(&check_output.stderr),
    );

    let test_output = cargo(copy)
        .args(["test", "-p", "north-star"])
        .output()
        .expect("cargo test runs");
    let test_stdout = String::from_utf8_lossy(&test_output.stdout);
    let test_stderr = String::from_utf8_lossy(&test_output.stderr);
    assert!(
        test_output.status.success(),
        "cargo test -p north-star failed:\nstdout:\n{test_stdout}\nstderr:\n{test_stderr}",
    );
    assert!(
        test_stdout.contains("opens_one_window_without_errors ... ok"),
        "expected `cargo test -p north-star` to run `opens_one_window_without_errors ... ok`:\nstdout:\n{test_stdout}\nstderr:\n{test_stderr}",
    );

    let stages_output = cargo(copy)
        .env_remove("YARD_PROMOTE")
        .args(["test", "-p", "yard", "--test", "stages"])
        .output()
        .expect("cargo test -p yard --test stages runs");
    assert!(
        stages_output.status.success(),
        "cargo test -p yard --test stages failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&stages_output.stdout),
        String::from_utf8_lossy(&stages_output.stderr),
    );

    // Register a small extension that logs both `tracing::error!` and
    // `log::error!` in its `init`, and require `cargo test -p north-star` to
    // fail with both messages in the output.
    const TRACE_ERROR: &str = "startup tracing probe failed";
    const LOG_ERROR: &str = "startup diagnostic probe failed";

    let app_manifest_path = made.folder.join("Cargo.toml");
    let app_manifest = fs::read_to_string(&app_manifest_path).unwrap();
    let parsed_manifest: toml::Table = toml::from_str(&app_manifest).unwrap();
    let deps_table = parsed_manifest["dependencies"].as_table().unwrap();
    let mut extra_deps = String::new();
    if !deps_table.contains_key("log") {
        extra_deps.push_str("log.workspace = true\n");
    }
    if !deps_table.contains_key("tracing") {
        extra_deps.push_str("tracing.workspace = true\n");
    }
    if !extra_deps.is_empty() {
        let deps_header = app_manifest
            .find("[dependencies]")
            .unwrap_or_else(|| panic!("expected `[dependencies]` in Cargo.toml:\n{app_manifest}"));
        let insert_at = app_manifest[deps_header..]
            .find('\n')
            .map(|offset| deps_header + offset + 1)
            .unwrap_or(app_manifest.len());
        let mut updated_manifest = String::with_capacity(app_manifest.len() + extra_deps.len());
        updated_manifest.push_str(&app_manifest[..insert_at]);
        updated_manifest.push_str(&extra_deps);
        updated_manifest.push_str(&app_manifest[insert_at..]);
        fs::write(&app_manifest_path, updated_manifest).unwrap();
    }

    let startup_path = made.folder.join("src/startup.rs");
    let startup_source = fs::read_to_string(&startup_path).unwrap();
    let extensions_decl = startup_source
        .find("pub const EXTENSIONS")
        .unwrap_or_else(|| {
            panic!("expected `pub const EXTENSIONS` in src/startup.rs:\n{startup_source}")
        });
    let eq_pos = startup_source[extensions_decl..]
        .find('=')
        .map(|offset| extensions_decl + offset)
        .unwrap_or_else(|| {
            panic!("expected `=` in EXTENSIONS in src/startup.rs:\n{startup_source}")
        });
    let slice_start = startup_source[eq_pos..]
        .find("&[")
        .map(|offset| eq_pos + offset + 2)
        .unwrap_or_else(|| {
            panic!("expected `&[` in EXTENSIONS in src/startup.rs:\n{startup_source}")
        });
    let mut updated_startup = String::with_capacity(startup_source.len() + 256);
    updated_startup.push_str(&startup_source[..slice_start]);
    updated_startup.push_str("(\"error_probe\", error_probe_init), ");
    updated_startup.push_str(&startup_source[slice_start..]);
    updated_startup.push_str(&format!(
        "\nfn error_probe_init(_cx: &mut gpui::App, _extension: &yard_shell::Extension) {{\n    tracing::error!({TRACE_ERROR:?});\n    log::error!({LOG_ERROR:?});\n}}\n"
    ));
    fs::write(&startup_path, updated_startup).unwrap();

    let failing_output = cargo(copy)
        .args(["test", "-p", "north-star"])
        .output()
        .expect("cargo test runs after adding error probe extension");
    let failing_stdout = String::from_utf8_lossy(&failing_output.stdout);
    let failing_stderr = String::from_utf8_lossy(&failing_output.stderr);
    assert!(
        !failing_output.status.success(),
        "expected `cargo test -p north-star` to fail once an extension logs errors in `init`:\nstdout:\n{failing_stdout}\nstderr:\n{failing_stderr}",
    );
    assert!(
        failing_stdout.contains("opens_one_window_without_errors ... FAILED"),
        "expected `opens_one_window_without_errors` to run and fail:\nstdout:\n{failing_stdout}\nstderr:\n{failing_stderr}",
    );
    let combined = format!("{failing_stdout}\n{failing_stderr}");
    assert!(
        combined.contains(TRACE_ERROR),
        "expected failure output to contain tracing error {TRACE_ERROR:?}:\nstdout:\n{failing_stdout}\nstderr:\n{failing_stderr}",
    );
    assert!(
        combined.contains(LOG_ERROR),
        "expected failure output to contain diagnostic log error {LOG_ERROR:?}:\nstdout:\n{failing_stdout}\nstderr:\n{failing_stderr}",
    );
}
