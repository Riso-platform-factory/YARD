#![cfg(feature = "dev")]
//! `bundle_target` picks the workspace package and its one binary from `cargo
//! metadata --no-deps --format-version 1`, and refuses anything ambiguous,
//! unknown, without a single binary, or asking for `--dev` on a package other
//! than `yard`.

use std::path::PathBuf;
use std::process::Command;

use serde_json::{Value, json};
use xtask::{Target, bundle_target};

/// Builds a minimal `cargo metadata --no-deps --format-version 1` JSON payload.
fn metadata(packages: &[Value], members: &[&str], defaults: &[&str]) -> Vec<u8> {
    json!({
        "packages": packages,
        "workspace_members": members,
        "workspace_default_members": defaults,
    })
    .to_string()
    .into_bytes()
}

fn package(name: &str, id: &str, manifest_path: &str, targets: &[(&str, &[&str])]) -> Value {
    let targets: Vec<Value> = targets
        .iter()
        .map(|(target_name, kinds)| json!({ "name": target_name, "kind": kinds }))
        .collect();
    json!({
        "name": name,
        "id": id,
        "manifest_path": manifest_path,
        "targets": targets,
    })
}

fn workspace_metadata() -> Vec<u8> {
    let yard_id = "path+file:///workspace/app/yard#0.1.0";
    let north_star_id = "path+file:///workspace/examples/north_star#0.1.0";
    metadata(
        &[
            package(
                "yard",
                yard_id,
                "/workspace/app/yard/Cargo.toml",
                &[("yard", &["lib"]), ("yard", &["bin"])],
            ),
            package(
                "north_star",
                north_star_id,
                "/workspace/examples/north_star/Cargo.toml",
                &[("north_star", &["lib"]), ("north_star", &["bin"])],
            ),
        ],
        &[yard_id, north_star_id],
        &[yard_id],
    )
}

#[test]
fn default_member_is_chosen_without_package() {
    let json = workspace_metadata();
    let target = bundle_target(&json, None, false).unwrap();
    assert_eq!(
        target,
        Target {
            package: "yard".into(),
            binary: "yard".into(),
            folder: PathBuf::from("/workspace/app/yard"),
        }
    );
}

#[test]
fn package_option_is_chosen_over_the_default_member() {
    let json = workspace_metadata();
    let target = bundle_target(&json, Some("north_star"), false).unwrap();
    assert_eq!(
        target,
        Target {
            package: "north_star".into(),
            binary: "north_star".into(),
            folder: PathBuf::from("/workspace/examples/north_star"),
        }
    );
}

#[test]
fn unknown_or_non_member_package_is_refused_naming_it() {
    let yard_id = "path+file:///workspace/app/yard#0.1.0";
    let dep_id = "registry+https://github.com/rust-lang/crates.io-index#dep_cli@0.1.0";
    let json = metadata(
        &[
            package(
                "yard",
                yard_id,
                "/workspace/app/yard/Cargo.toml",
                &[("yard", &["bin"])],
            ),
            package(
                "dep_cli",
                dep_id,
                "/cargo/registry/dep_cli/Cargo.toml",
                &[("dep_cli", &["bin"])],
            ),
        ],
        &[yard_id],
        &[yard_id],
    );

    for name in ["no_such_app", "dep_cli"] {
        let reason = bundle_target(&json, Some(name), false).unwrap_err();
        assert!(
            reason.contains(name),
            "refusal should name {name:?}: {reason}"
        );
    }
}

#[test]
fn zero_or_several_default_members_are_refused_naming_the_package_option() {
    let yard_id = "path+file:///workspace/app/yard#0.1.0";
    let north_star_id = "path+file:///workspace/examples/north_star#0.1.0";
    let packages = [
        package(
            "yard",
            yard_id,
            "/workspace/app/yard/Cargo.toml",
            &[("yard", &["bin"])],
        ),
        package(
            "north_star",
            north_star_id,
            "/workspace/examples/north_star/Cargo.toml",
            &[("north_star", &["bin"])],
        ),
    ];

    for defaults in [&[][..], &[yard_id, north_star_id][..]] {
        let json = metadata(&packages, &[yard_id, north_star_id], defaults);
        let reason = bundle_target(&json, None, false).unwrap_err();
        assert!(
            reason.contains("--package"),
            "refusal for defaults {defaults:?} should name --package: {reason}"
        );
        // Naming a package explicitly still succeeds.
        let target = bundle_target(&json, Some("north_star"), false).unwrap();
        assert_eq!(target.package, "north_star");
    }
}

#[test]
fn package_with_no_binary_or_multiple_binaries_is_refused_naming_it() {
    let lib_id = "path+file:///workspace/crates/lib_only#0.1.0";
    let two_id = "path+file:///workspace/app/two_bins#0.1.0";
    let json = metadata(
        &[
            package(
                "lib_only",
                lib_id,
                "/workspace/crates/lib_only/Cargo.toml",
                &[("lib_only", &["lib"]), ("integration", &["test"])],
            ),
            package(
                "two_bins",
                two_id,
                "/workspace/app/two_bins/Cargo.toml",
                &[("alpha", &["bin"]), ("beta", &["bin"])],
            ),
        ],
        &[lib_id, two_id],
        &[],
    );

    for name in ["lib_only", "two_bins"] {
        let reason = bundle_target(&json, Some(name), false).unwrap_err();
        assert!(
            reason.contains(name),
            "refusal should name {name:?}: {reason}"
        );
    }
}

#[test]
fn non_binary_targets_beside_the_one_binary_are_ignored() {
    let app_id = "path+file:///workspace/app/compass#0.1.0";
    let json = metadata(
        &[package(
            "compass",
            app_id,
            "/workspace/app/compass/Cargo.toml",
            &[
                ("compass", &["lib"]),
                ("build-script-build", &["custom-build"]),
                ("smoke", &["test"]),
                ("demo", &["example"]),
                ("compass-app", &["bin"]),
            ],
        )],
        &[app_id],
        &[app_id],
    );

    let target = bundle_target(&json, None, false).unwrap();
    assert_eq!(
        target,
        Target {
            package: "compass".into(),
            binary: "compass-app".into(),
            folder: PathBuf::from("/workspace/app/compass"),
        }
    );
}

#[test]
fn dev_is_accepted_for_yard_and_refused_for_other_packages() {
    let json = workspace_metadata();
    let default_dev = bundle_target(&json, None, true).unwrap();
    assert_eq!(default_dev.package, "yard");
    let explicit_dev = bundle_target(&json, Some("yard"), true).unwrap();
    assert_eq!(explicit_dev.package, "yard");

    let reason = bundle_target(&json, Some("north_star"), true).unwrap_err();
    assert!(
        reason.contains("--dev"),
        "dev refusal should mention --dev: {reason}"
    );

    // Even as the sole default member, a package other than `yard` has no `--dev`.
    let north_star_id = "path+file:///workspace/examples/north_star#0.1.0";
    let only_north_star = metadata(
        &[package(
            "north_star",
            north_star_id,
            "/workspace/examples/north_star/Cargo.toml",
            &[("north_star", &["bin"])],
        )],
        &[north_star_id],
        &[north_star_id],
    );
    let reason = bundle_target(&only_north_star, None, true).unwrap_err();
    assert!(
        reason.contains("--dev"),
        "dev refusal should mention --dev: {reason}"
    );
}

#[test]
fn real_workspace_resolves_yard_by_default_and_north_star_by_name() {
    let repo = xtask::repository();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .current_dir(&repo)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let default = bundle_target(&output.stdout, None, false).unwrap();
    assert_eq!(default.package, "yard");
    assert_eq!(default.binary, "yard");
    assert_eq!(
        default.folder.canonicalize().unwrap(),
        repo.join("app/yard").canonicalize().unwrap()
    );

    let default_dev = bundle_target(&output.stdout, None, true).unwrap();
    assert_eq!(default_dev, default);

    let north_star = bundle_target(&output.stdout, Some("north_star"), false).unwrap();
    assert_eq!(north_star.package, "north_star");
    assert_eq!(north_star.binary, "north_star");
    assert_eq!(
        north_star.folder.canonicalize().unwrap(),
        repo.join("examples/north_star").canonicalize().unwrap()
    );

    assert!(bundle_target(&output.stdout, Some("north_star"), true).is_err());
}
