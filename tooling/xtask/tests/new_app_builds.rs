#![cfg(feature = "dev")]
//! End-to-end check that `new_app` generates an application that Cargo resolves
//! as the workspace's default member and compiles cleanly inside a copy of the
//! real repository.

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
}
