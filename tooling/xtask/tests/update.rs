#![cfg(feature = "dev")]
//! Acceptance tests for part 1 of `upstream-updates`: `cargo xtask update` on a
//! repository that shares YARD's history.

#[allow(dead_code)]
mod support;

use std::fs;
use std::path::Path;

use support::update::{
    RepoState, UpdateFixture, UpdateRunner, cargo_generate_lockfile, commit_message,
    commit_parents, configure_git_repo, current_branch, file_url, git, git_commit_all, git_init,
    git_status, is_working_tree_clean, local_branches, lock_packages, merge_in_progress, rev_oid,
    snapshot_dir, tree_oid, write_file,
};
use xtask::{UpdateArgs, Updated, YARD_REMOTE, update};

fn args_for(fixture: &UpdateFixture) -> UpdateArgs {
    UpdateArgs {
        reference: "main".into(),
        remote: Some(fixture.yard_remote()),
        base: None,
        resume: false,
    }
}

fn manifest_table(root: &Path) -> toml::Table {
    let text = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    toml::from_str(&text).unwrap()
}

fn string_array(table: &toml::Table, section: &str, key: &str) -> Vec<String> {
    table[section][key]
        .as_array()
        .unwrap_or_else(|| panic!("expected array at [{section}].{key}"))
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn yard_remote_constant_matches_contract() {
    assert_eq!(
        YARD_REMOTE,
        "https://github.com/Riso-platform-factory/YARD.git"
    );
}

#[test]
fn updates_to_newer_yard_commit() {
    let fixture = UpdateFixture::standard();
    let north_star_before = snapshot_dir(&fixture.dev.join("app/north-star"), false);
    let main_tree_before = tree_oid(&fixture.dev, "main");
    let yard_lock = lock_packages(&fixture.yard.join("Cargo.lock"));
    let expected_branch = fixture.expected_branch();

    let mut runner = UpdateRunner::new();
    let outcome = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap();

    match outcome {
        Updated::Done {
            branch,
            yard_commit,
            start,
            carried,
            lock_changes,
        } => {
            assert_eq!(branch, expected_branch);
            assert_eq!(yard_commit, fixture.yard_commit);
            assert_eq!(start, "main");
            assert!(
                carried.is_empty(),
                "no YARD file was modified by the developer: {carried:?}"
            );
            assert!(
                !lock_changes.is_empty(),
                "expected lock changes to be reported when YARD updated engine and added helper"
            );
        }
        other => panic!("expected Updated::Done, got {other:?}"),
    }

    // Update branch exists, is checked out, and holds a clean merge commit.
    assert!(local_branches(&fixture.dev).contains(&expected_branch));
    assert_eq!(
        current_branch(&fixture.dev).as_deref(),
        Some(expected_branch.as_str())
    );
    assert!(!merge_in_progress(&fixture.dev));
    assert!(is_working_tree_clean(&fixture.dev));
    assert_eq!(
        commit_parents(&fixture.dev, "HEAD"),
        vec![fixture.dev_commit.clone(), fixture.yard_commit.clone()],
        "merge commit must have the developer commit and YARD commit as parents"
    );
    assert!(
        commit_message(&fixture.dev, "HEAD").contains(&fixture.yard_commit),
        "merge commit message must contain the full YARD commit"
    );

    // YARD's new engine and helper changes are present.
    assert_eq!(
        fs::read_to_string(fixture.dev.join("crates/engine/src/lib.rs")).unwrap(),
        fs::read_to_string(fixture.yard.join("crates/engine/src/lib.rs")).unwrap()
    );
    assert_eq!(
        fs::read_to_string(fixture.dev.join("crates/helper/src/lib.rs")).unwrap(),
        fs::read_to_string(fixture.yard.join("crates/helper/src/lib.rs")).unwrap()
    );

    // Developer's application folder is byte-identical.
    assert_eq!(
        snapshot_dir(&fixture.dev.join("app/north-star"), false),
        north_star_before,
        "app/north-star/ must be byte-identical after update"
    );

    // Root Cargo.toml is YARD's new file plus the developer's default-members and members entry.
    let merged_manifest_text = fs::read_to_string(fixture.dev.join("Cargo.toml")).unwrap();
    assert!(
        merged_manifest_text.contains("# YARD workspace manifest v2"),
        "root Cargo.toml must start from YARD's new file:\n{merged_manifest_text}"
    );
    let merged_manifest = manifest_table(&fixture.dev);
    assert_eq!(
        string_array(&merged_manifest, "workspace", "default-members"),
        vec!["app/north-star"]
    );
    assert_eq!(
        string_array(&merged_manifest, "workspace", "members"),
        vec!["app/*", "crates/*", "app/north-star/extensions/*"]
    );
    assert_eq!(
        merged_manifest["workspace"]["dependencies"],
        manifest_table(&fixture.yard)["workspace"]["dependencies"],
        "workspace.dependencies must match YARD's new manifest"
    );

    // Cargo.lock holds every YARD package unchanged plus the developer's packages.
    let merged_lock = lock_packages(&fixture.dev.join("Cargo.lock"));
    for yard_pkg in &yard_lock {
        assert!(
            merged_lock.contains(yard_pkg),
            "YARD package {yard_pkg:?} must remain unchanged in merged Cargo.lock: {merged_lock:?}"
        );
    }
    assert!(
        merged_lock
            .iter()
            .any(|(name, version, _)| name == "north-star" && version == "0.1.0"),
        "north-star must be present in merged Cargo.lock: {merged_lock:?}"
    );
    assert!(
        merged_lock
            .iter()
            .any(|(name, version, _)| name == "notes" && version == "0.1.0"),
        "notes must be present in merged Cargo.lock: {merged_lock:?}"
    );

    // Developer's original branch is untouched.
    assert_eq!(rev_oid(&fixture.dev, "main"), fixture.dev_commit);
    assert_eq!(tree_oid(&fixture.dev, "main"), main_tree_before);

    // Remote recorded, no push, checks and metadata ran through the runner.
    assert_eq!(
        git(&fixture.dev, &["remote", "get-url", "yard-upstream"]),
        fixture.yard_remote()
    );
    assert!(runner.ran_git("fetch"), "expected git fetch to run");
    assert!(
        !runner.ran_git("push"),
        "update must never push: {:?}",
        runner.commands
    );
    assert!(
        runner.ran_cargo_metadata(),
        "expected cargo metadata --format-version 1 to run: {:?}",
        runner.commands
    );
    assert!(
        runner.ran_cargo_test_yard(),
        "expected cargo test -p yard to run: {:?}",
        runner.commands
    );
    assert!(
        runner.ran_cargo_check_locked(),
        "expected cargo check --workspace --all-targets --locked to run: {:?}",
        runner.commands
    );
}

#[test]
fn second_update_after_merging_is_up_to_date() {
    let fixture = UpdateFixture::standard();
    let args = args_for(&fixture);

    let mut first_runner = UpdateRunner::new();
    let Updated::Done { branch, .. } = update(&fixture.dev, &args, &mut first_runner).unwrap()
    else {
        panic!("expected Updated::Done on first run");
    };

    git(&fixture.dev, &["checkout", "main"]);
    git(&fixture.dev, &["merge", &branch]);
    let state_before = RepoState::capture(&fixture.dev);

    let mut second_runner = UpdateRunner::new();
    let outcome = update(&fixture.dev, &args, &mut second_runner).unwrap();
    assert_eq!(
        outcome,
        Updated::UpToDate {
            yard_commit: fixture.yard_commit.clone(),
        }
    );
    assert_eq!(RepoState::capture(&fixture.dev), state_before);
    assert!(!second_runner.ran_cargo_test_yard());
    assert!(!second_runner.ran_cargo_check_locked());
    assert!(!second_runner.ran_git("push"));
}

#[test]
fn keeps_developer_change_to_untouched_yard_file_and_names_it_in_carried() {
    let fixture = UpdateFixture::standard();
    let custom_doc = "# YARD Docs\n\nLine one.\nLine two.\nLine three.\nDeveloper note.\n";
    write_file(&fixture.dev.join("docs/x.md"), custom_doc);
    git_commit_all(&fixture.dev, "developer note in docs/x.md");

    let mut runner = UpdateRunner::new();
    let outcome = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap();
    match outcome {
        Updated::Done { carried, .. } => {
            assert_eq!(carried, vec!["docs/x.md".to_owned()]);
        }
        other => panic!("expected Updated::Done, got {other:?}"),
    }

    assert_eq!(
        fs::read_to_string(fixture.dev.join("docs/x.md")).unwrap(),
        custom_doc
    );
}

#[test]
fn stops_on_conflicting_yard_file_and_resumes_with_continue() {
    let mut fixture = UpdateFixture::at_base();

    // Developer edits Line one in docs/x.md.
    write_file(
        &fixture.dev.join("docs/x.md"),
        "# YARD Docs\n\nLine one from developer.\nLine two.\nLine three.\n",
    );
    let dev_head = git_commit_all(&fixture.dev, "edit docs/x.md on developer side");
    let north_star_before = snapshot_dir(&fixture.dev.join("app/north-star"), false);

    // YARD v2 edits the same line in docs/x.md.
    write_file(
        &fixture.yard.join("docs/x.md"),
        "# YARD Docs\n\nLine one from YARD.\nLine two.\nLine three.\n",
    );
    fixture.advance_yard_standard();
    let expected_branch = fixture.expected_branch();

    let mut first_runner = UpdateRunner::new();
    let first = update(&fixture.dev, &args_for(&fixture), &mut first_runner).unwrap();
    match first {
        Updated::Conflicts {
            branch,
            paths,
            manifest_keys,
        } => {
            assert_eq!(branch, expected_branch);
            assert_eq!(paths, vec!["docs/x.md".to_owned()]);
            assert!(
                manifest_keys.is_empty(),
                "no manifest conflicts expected: {manifest_keys:?}"
            );
        }
        other => panic!("expected Updated::Conflicts, got {other:?}"),
    }

    // Merge is in progress on the update branch; main is untouched; checks did not run.
    assert_eq!(
        current_branch(&fixture.dev).as_deref(),
        Some(expected_branch.as_str())
    );
    assert!(merge_in_progress(&fixture.dev));
    assert_eq!(rev_oid(&fixture.dev, "main"), dev_head);
    assert!(!first_runner.ran_cargo_test_yard());
    assert!(!first_runner.ran_cargo_check_locked());

    // Developer resolves the conflict and stages it, committing nothing.
    let resolved = "# YARD Docs\n\nLine one merged.\nLine two.\nLine three.\n";
    write_file(&fixture.dev.join("docs/x.md"), resolved);
    git(&fixture.dev, &["add", "docs/x.md"]);
    assert!(merge_in_progress(&fixture.dev));

    let resume_args = UpdateArgs {
        reference: "main".into(),
        remote: None,
        base: None,
        resume: true,
    };
    let mut resume_runner = UpdateRunner::new();
    let resumed = update(&fixture.dev, &resume_args, &mut resume_runner).unwrap();
    match resumed {
        Updated::Done {
            branch,
            yard_commit,
            start,
            ..
        } => {
            assert_eq!(branch, expected_branch);
            assert_eq!(yard_commit, fixture.yard_commit);
            assert_eq!(start, "main");
        }
        other => panic!("expected Updated::Done after --continue, got {other:?}"),
    }

    assert!(!merge_in_progress(&fixture.dev));
    assert!(is_working_tree_clean(&fixture.dev));
    assert_eq!(
        commit_parents(&fixture.dev, "HEAD"),
        vec![dev_head.clone(), fixture.yard_commit.clone()]
    );
    assert!(commit_message(&fixture.dev, "HEAD").contains(&fixture.yard_commit));
    assert_eq!(
        fs::read_to_string(fixture.dev.join("docs/x.md")).unwrap(),
        resolved
    );
    assert_eq!(
        snapshot_dir(&fixture.dev.join("app/north-star"), false),
        north_star_before
    );
    let yard_lock = lock_packages(&fixture.yard.join("Cargo.lock"));
    let merged_lock = lock_packages(&fixture.dev.join("Cargo.lock"));
    for yard_pkg in &yard_lock {
        assert!(
            merged_lock.contains(yard_pkg),
            "YARD package {yard_pkg:?} must be present after --continue: {merged_lock:?}"
        );
    }
    assert!(
        merged_lock
            .iter()
            .any(|(name, version, _)| name == "north-star" && version == "0.1.0"),
        "north-star must be present in Cargo.lock after --continue: {merged_lock:?}"
    );
    assert_eq!(rev_oid(&fixture.dev, "main"), dev_head);
    assert!(resume_runner.ran_cargo_test_yard());
    assert!(resume_runner.ran_cargo_check_locked());
    assert!(!resume_runner.ran_git("push"));
}

#[test]
fn continue_ignores_unstaged_cargo_lock_modification() {
    let mut fixture = UpdateFixture::at_base();
    write_file(&fixture.dev.join("docs/x.md"), "Developer docs.\n");
    git_commit_all(&fixture.dev, "edit developer docs");
    write_file(&fixture.yard.join("docs/x.md"), "YARD docs.\n");
    fixture.advance_yard_standard();

    let args = args_for(&fixture);
    let first = update(&fixture.dev, &args, &mut UpdateRunner::new()).unwrap();
    assert!(
        matches!(first, Updated::Conflicts { ref paths, .. } if paths == &["docs/x.md"]),
        "{first:?}"
    );

    write_file(&fixture.dev.join("docs/x.md"), "Resolved docs.\n");
    git(&fixture.dev, &["add", "docs/x.md"]);
    let lock_path = fixture.dev.join("Cargo.lock");
    let lock = fs::read_to_string(&lock_path).unwrap();
    write_file(&lock_path, &format!("{lock}\n# Unstaged comment\n"));
    assert_eq!(
        git(&fixture.dev, &["diff", "--name-only", "--", "Cargo.lock"]),
        "Cargo.lock"
    );

    let resume_args = UpdateArgs {
        resume: true,
        ..args
    };
    let resumed = update(&fixture.dev, &resume_args, &mut UpdateRunner::new()).unwrap();
    assert!(matches!(resumed, Updated::Done { .. }), "{resumed:?}");

    let mut expected = lock_packages(&fixture.yard.join("Cargo.lock"));
    expected.push(("north-star".into(), "0.1.0".into(), None));
    expected.push(("notes".into(), "0.1.0".into(), None));
    expected.sort();
    assert_eq!(lock_packages(&lock_path), expected);
}

#[test]
fn developer_patch_in_root_manifest_is_a_conflict() {
    let fixture = UpdateFixture::standard();
    let mut manifest = fs::read_to_string(fixture.dev.join("Cargo.toml")).unwrap();
    manifest.push_str("\n[patch.crates-io]\nengine = { path = \"crates/engine\" }\n");
    write_file(&fixture.dev.join("Cargo.toml"), &manifest);
    git_commit_all(&fixture.dev, "add [patch] in root manifest");

    let mut runner = UpdateRunner::new();
    let outcome = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap();
    match outcome {
        Updated::Conflicts {
            branch,
            manifest_keys,
            ..
        } => {
            assert_eq!(branch, fixture.expected_branch());
            assert!(
                manifest_keys.iter().any(|key| key.contains("patch")),
                "expected a patch key in manifest_keys: {manifest_keys:?}"
            );
        }
        other => panic!("expected Updated::Conflicts for [patch], got {other:?}"),
    }

    assert_eq!(
        current_branch(&fixture.dev).as_deref(),
        Some(fixture.expected_branch().as_str())
    );
    assert!(merge_in_progress(&fixture.dev));
    assert!(!runner.ran_cargo_test_yard());
    assert!(!runner.ran_cargo_check_locked());
}

#[test]
fn resumes_after_root_manifest_conflict_keeping_developer_profile() {
    let mut fixture = UpdateFixture::at_base();
    let yard_path = fixture.yard.join("Cargo.toml");
    let yard_manifest =
        fs::read_to_string(&yard_path).unwrap() + "\n[profile.dev]\nopt-level = 1\n";
    write_file(&yard_path, &yard_manifest);
    fixture.yard_commit = git_commit_all(&fixture.yard, "add YARD profile.dev");
    let mut manifest = fs::read_to_string(fixture.dev.join("Cargo.toml")).unwrap();
    manifest.push_str("\n[profile.release]\nlto = \"thin\"\n");
    write_file(&fixture.dev.join("Cargo.toml"), &manifest);
    git_commit_all(&fixture.dev, "add [profile.release] in root manifest");

    let mut runner = UpdateRunner::new();
    let outcome = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap();
    match outcome {
        Updated::Conflicts {
            branch,
            manifest_keys,
            ..
        } => {
            assert_eq!(branch, fixture.expected_branch());
            assert!(
                manifest_keys.iter().any(|key| key.contains("profile")),
                "expected a profile key in manifest_keys: {manifest_keys:?}"
            );
        }
        other => panic!("expected Updated::Conflicts for [profile], got {other:?}"),
    }

    assert!(merge_in_progress(&fixture.dev));

    let resume_args = UpdateArgs {
        resume: true,
        ..args_for(&fixture)
    };
    let mut unresolved_runner = UpdateRunner::new();
    let unresolved = update(&fixture.dev, &resume_args, &mut unresolved_runner).unwrap();
    assert!(
        matches!(unresolved, Updated::Conflicts { .. }),
        "expected conflict until Cargo.toml is staged, got {unresolved:?}"
    );
    assert!(merge_in_progress(&fixture.dev));

    git(&fixture.dev, &["add", "Cargo.toml"]);
    let kept_ours = update(&fixture.dev, &resume_args, &mut UpdateRunner::new()).unwrap();
    assert!(
        matches!(&kept_ours, Updated::Conflicts { manifest_keys, .. }
            if manifest_keys.iter().any(|key| key == "profile.dev.opt-level = 1")),
        "{kept_ours:?}"
    );

    let resolved = fs::read_to_string(fixture.yard.join("Cargo.toml"))
        .unwrap()
        .replace(
            r#"members = ["app/*", "crates/*"]"#,
            r#"members = ["app/*", "crates/*", "app/north-star/extensions/*"]"#,
        )
        .replace(
            r#"default-members = ["app/yard"]"#,
            r#"default-members = ["app/north-star"]"#,
        )
        + "\n[profile.release]\nlto = \"thin\"\n";
    write_file(&fixture.dev.join("Cargo.toml"), &resolved);
    git(&fixture.dev, &["add", "Cargo.toml"]);

    let mut resume_runner = UpdateRunner::new();
    let resumed = update(&fixture.dev, &resume_args, &mut resume_runner).unwrap();
    assert!(matches!(resumed, Updated::Done { .. }), "{resumed:?}");
    assert!(is_working_tree_clean(&fixture.dev));
    assert_eq!(
        fs::read_to_string(fixture.dev.join("Cargo.toml")).unwrap(),
        resolved
    );
    assert!(resume_runner.ran_cargo_test_yard());
    assert!(resume_runner.ran_cargo_check_locked());
}

#[test]
fn staging_theirs_keeps_developer_workspace_entries() {
    let fixture = UpdateFixture::standard();
    let path = fixture.dev.join("Cargo.toml");
    let manifest = fs::read_to_string(&path).unwrap() + "\n[profile.release]\nlto = \"thin\"\n";
    write_file(&path, &manifest);
    git_commit_all(&fixture.dev, "add developer profile");
    let mut resume_args = args_for(&fixture);
    let outcome = update(&fixture.dev, &resume_args, &mut UpdateRunner::new()).unwrap();
    assert!(matches!(outcome, Updated::Conflicts { .. }), "{outcome:?}");
    resume_args.resume = true;
    git(&fixture.dev, &["checkout", "--theirs", "--", "Cargo.toml"]);
    git(&fixture.dev, &["add", "--", "Cargo.toml"]);
    let kept_theirs = update(&fixture.dev, &resume_args, &mut UpdateRunner::new()).unwrap();
    assert!(
        matches!(&kept_theirs, Updated::Conflicts { manifest_keys, .. }
            if manifest_keys.iter().any(|key| key.contains("workspace.default-members"))
            && manifest_keys.iter().any(|key| key.contains("app/north-star/extensions/*"))),
        "{kept_theirs:?}"
    );
    assert!(merge_in_progress(&fixture.dev));
}

#[test]
fn developer_workspace_dependency_in_root_manifest_is_a_conflict() {
    let fixture = UpdateFixture::standard();
    let mut manifest = fs::read_to_string(fixture.dev.join("Cargo.toml")).unwrap();
    manifest.push_str("notes = { path = \"app/north-star/extensions/notes\" }\n");
    write_file(&fixture.dev.join("Cargo.toml"), &manifest);
    git_commit_all(&fixture.dev, "add workspace dependency in root manifest");

    let mut runner = UpdateRunner::new();
    let outcome = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap();
    match outcome {
        Updated::Conflicts {
            branch,
            manifest_keys,
            ..
        } => {
            assert_eq!(branch, fixture.expected_branch());
            assert!(
                manifest_keys
                    .iter()
                    .any(|key| key.contains("workspace.dependencies") || key.contains("notes")),
                "expected workspace.dependencies/notes key in manifest_keys: {manifest_keys:?}"
            );
        }
        other => {
            panic!("expected Updated::Conflicts for [workspace.dependencies], got {other:?}")
        }
    }

    assert!(merge_in_progress(&fixture.dev));
}

#[test]
fn adds_explicit_app_member_when_yard_drops_app_glob() {
    let mut fixture = UpdateFixture::at_base();
    write_file(
        &fixture.yard.join("Cargo.toml"),
        "# YARD workspace manifest without app/* glob\n\
         [workspace]\n\
         resolver = \"3\"\n\
         members = [\"app/yard\", \"crates/*\"]\n\
         default-members = [\"app/yard\"]\n\n\
         [workspace.package]\n\
         edition = \"2024\"\n\
         publish = false\n\
         license = \"GPL-3.0-or-later\"\n\n\
         [workspace.dependencies]\n\
         engine = { path = \"crates/engine\" }\n",
    );
    write_file(
        &fixture.yard.join("crates/engine/src/lib.rs"),
        "pub fn version() -> &'static str {\n    \"2\"\n}\n",
    );
    cargo_generate_lockfile(&fixture.yard, true);
    fixture.yard_commit = git_commit_all(&fixture.yard, "drop app/* glob from members");

    let mut runner = UpdateRunner::new();
    let outcome = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap();
    assert!(matches!(outcome, Updated::Done { .. }), "{outcome:?}");

    let merged = manifest_table(&fixture.dev);
    let members = string_array(&merged, "workspace", "members");
    assert!(
        !members.contains(&"app/*".to_owned()),
        "app/* should not be re-introduced: {members:?}"
    );
    assert!(
        members.contains(&"app/yard".to_owned()),
        "YARD's app/yard member should stay: {members:?}"
    );
    assert!(
        members.contains(&"app/north-star".to_owned()),
        "expected explicit \"app/north-star\" when YARD dropped app/*: {members:?}"
    );
    assert!(
        members.contains(&"app/north-star/extensions/*".to_owned()),
        "expected developer's extension glob to stay: {members:?}"
    );
    assert_eq!(
        string_array(&merged, "workspace", "default-members"),
        vec!["app/north-star"]
    );
}

#[test]
fn refuses_when_application_moves_a_yard_lock_pin() {
    let fixture = UpdateFixture::moved_pin();
    let mut runner = UpdateRunner::new();

    let failure = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap_err();
    assert_eq!(failure.step, "lock", "unexpected failure: {failure}");
    assert!(
        failure.is_refusal(),
        "a moved YARD pin must be a refusal: {failure}"
    );
    assert!(
        failure.reason.contains("shared_dep"),
        "failure reason must name the moved package: {}",
        failure.reason
    );
    assert!(
        failure.reason.contains("1.0.0"),
        "failure reason must name YARD's pinned version: {}",
        failure.reason
    );
    assert!(
        failure.reason.contains("north-star")
            && failure
                .reason
                .split(|character: char| !character.is_ascii_alphanumeric() && character != '.')
                .any(|word| word.trim_end_matches('.') == "1.1"),
        "failure reason must name north-star and its 1.1 requirement: {}",
        failure.reason
    );
    assert_rolled_back(&fixture);
    assert!(
        failure
            .reason
            .contains("Change the named requirement on main")
    );
    run_recovery(&fixture, &failure.reason);
    let retry = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap_err();
    assert_eq!(retry.step, "lock");
    assert_eq!(rev_oid(&fixture.dev, "main"), fixture.dev_commit);
    assert!(!runner.ran_cargo_test_yard());
    assert!(!runner.ran_cargo_check_locked());
}

#[test]
fn refuses_yard_commit_adding_path_under_app_other() {
    let mut fixture = UpdateFixture::at_base();
    write_file(&fixture.yard.join("app/other/x"), "not allowed\n");
    fixture.yard_commit = git_commit_all(&fixture.yard, "illegal path under app/other");

    let before = RepoState::capture(&fixture.dev);
    let mut runner = UpdateRunner::new();
    let failure = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap_err();

    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert!(
        failure.reason.contains("app/other"),
        "failure should name the offending path: {}",
        failure.reason
    );
    assert_eq!(RepoState::capture(&fixture.dev), before);
    assert!(
        !runner.ran_git("merge"),
        "must refuse before merging: {:?}",
        runner.commands
    );
}

#[test]
fn refuses_refspec_in_ref_argument() {
    for reference in ["main:refs/heads/hijacked", "+main:main"] {
        let fixture = UpdateFixture::standard();
        let before = RepoState::capture(&fixture.dev);
        let args = UpdateArgs {
            reference: reference.into(),
            ..args_for(&fixture)
        };
        let mut runner = UpdateRunner::new();
        let outcome = update(&fixture.dev, &args, &mut runner);

        assert!(!runner.ran_git("fetch"), "fetched for {reference:?}");
        assert_eq!(RepoState::capture(&fixture.dev), before);
        assert!(
            !local_branches(&fixture.dev)
                .iter()
                .any(|branch| branch == "hijacked")
        );
        let failure = outcome.unwrap_err();
        assert_eq!(failure.step, "arguments");
        assert!(failure.is_refusal());
    }
}

#[test]
fn refuses_unrelated_history() {
    let fixture = UpdateFixture::standard();
    let unrelated = fixture.dev.with_file_name("unrelated");
    git_init(&unrelated);
    let prefix = format!("--prefix={}/", unrelated.display());
    git(&fixture.dev, &["checkout-index", "--all", &prefix]);
    git_commit_all(&unrelated, "independent workspace");
    let before = RepoState::capture(&unrelated);

    let failure = update(&unrelated, &args_for(&fixture), &mut UpdateRunner::new()).unwrap_err();

    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert_eq!(RepoState::capture(&unrelated), before);
}

#[test]
fn refuses_unstaged_working_tree_changes() {
    let fixture = UpdateFixture::standard();
    write_file(&fixture.dev.join("docs/x.md"), "unstaged modification\n");
    let before = RepoState::capture(&fixture.dev);

    let mut runner = UpdateRunner::new();
    let failure = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap_err();

    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert_eq!(RepoState::capture(&fixture.dev), before);
}

#[test]
fn refuses_staged_working_tree_changes() {
    let fixture = UpdateFixture::standard();
    write_file(&fixture.dev.join("docs/x.md"), "staged modification\n");
    git(&fixture.dev, &["add", "docs/x.md"]);
    let before = RepoState::capture(&fixture.dev);

    let mut runner = UpdateRunner::new();
    let failure = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap_err();

    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert_eq!(RepoState::capture(&fixture.dev), before);
}

#[test]
fn refuses_untracked_file() {
    let fixture = UpdateFixture::standard();
    write_file(&fixture.dev.join("untracked.txt"), "hello\n");
    let before = RepoState::capture(&fixture.dev);

    let mut runner = UpdateRunner::new();
    let failure = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap_err();

    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert_eq!(RepoState::capture(&fixture.dev), before);
}

#[test]
fn refuses_merge_in_progress() {
    let fixture = UpdateFixture::standard();
    git(&fixture.dev, &["checkout", "-b", "side"]);
    write_file(&fixture.dev.join("docs/x.md"), "side change\n");
    git_commit_all(&fixture.dev, "side edit");
    git(&fixture.dev, &["checkout", "main"]);
    write_file(&fixture.dev.join("docs/x.md"), "main change\n");
    git_commit_all(&fixture.dev, "main edit");
    let status = git_status(&fixture.dev, &["merge", "side"]);
    assert!(!status.success(), "expected a conflicting merge");
    assert!(merge_in_progress(&fixture.dev));
    let before = RepoState::capture(&fixture.dev);

    let mut runner = UpdateRunner::new();
    let failure = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap_err();

    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert_eq!(RepoState::capture(&fixture.dev), before);
}

#[test]
fn refuses_detached_head() {
    let fixture = UpdateFixture::standard();
    git(&fixture.dev, &["checkout", "--detach", "HEAD"]);
    assert!(current_branch(&fixture.dev).is_none());
    let before = RepoState::capture(&fixture.dev);

    let mut runner = UpdateRunner::new();
    let failure = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap_err();

    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert_eq!(RepoState::capture(&fixture.dev), before);
}

#[test]
fn refuses_shallow_clone() {
    let fixture = UpdateFixture::standard();
    let shallow = fixture.temp_path().join("dev-shallow");
    git(
        fixture.temp_path(),
        &[
            "clone",
            "--depth",
            "1",
            &file_url(&fixture.dev),
            &shallow.to_string_lossy(),
        ],
    );
    configure_git_repo(&shallow);
    assert_eq!(
        git(&shallow, &["rev-parse", "--is-shallow-repository"]),
        "true",
        "fixture must be a shallow clone"
    );
    let before = RepoState::capture(&shallow);

    let mut runner = UpdateRunner::new();
    let failure = update(&shallow, &args_for(&fixture), &mut runner).unwrap_err();

    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert!(
        failure.reason.contains("git fetch --unshallow"),
        "shallow refusal should tell the developer to run `git fetch --unshallow`: {}",
        failure.reason
    );
    assert_eq!(RepoState::capture(&shallow), before);
}

#[test]
fn refuses_when_already_on_an_update_branch() {
    let fixture = UpdateFixture::standard();
    git(
        &fixture.dev,
        &["checkout", "-b", "yard-update/0123456789ab"],
    );
    let before = RepoState::capture(&fixture.dev);

    let mut runner = UpdateRunner::new();
    let failure = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap_err();

    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert_eq!(RepoState::capture(&fixture.dev), before);
}

#[test]
fn refuses_when_update_branch_for_commit_already_exists() {
    let fixture = UpdateFixture::standard();
    let existing = fixture.expected_branch();
    git(&fixture.dev, &["branch", &existing]);
    let before = RepoState::capture(&fixture.dev);

    let mut runner = UpdateRunner::new();
    let failure = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap_err();

    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert!(
        failure.reason.contains(&existing),
        "refusal must name the existing update branch {existing}: {}",
        failure.reason
    );
    assert_eq!(RepoState::capture(&fixture.dev), before);
}

#[test]
fn refuses_when_yard_upstream_has_another_url() {
    let fixture = UpdateFixture::standard();
    let other_url = "https://example.invalid/other-yard.git";
    git(&fixture.dev, &["remote", "add", "yard-upstream", other_url]);
    let before = RepoState::capture(&fixture.dev);

    let mut runner = UpdateRunner::new();
    let failure = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap_err();

    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert!(
        failure.reason.contains("yard-upstream"),
        "refusal must name yard-upstream: {}",
        failure.reason
    );
    assert_eq!(
        git(&fixture.dev, &["remote", "get-url", "yard-upstream"]),
        other_url
    );
    assert_eq!(RepoState::capture(&fixture.dev), before);
    assert!(!runner.ran_git("fetch"));
}

#[test]
fn refuses_rewritten_upstream_history() {
    let fixture = UpdateFixture::standard();
    let args = args_for(&fixture);

    // Take YARD v2 and merge it into main.
    let mut first_runner = UpdateRunner::new();
    let Updated::Done {
        branch,
        yard_commit: old_yard_commit,
        ..
    } = update(&fixture.dev, &args, &mut first_runner).unwrap()
    else {
        panic!("expected Updated::Done on first update");
    };
    git(&fixture.dev, &["checkout", "main"]);
    git(&fixture.dev, &["merge", &branch]);

    // Rewrite YARD's main to a different commit branching off base_commit,
    // dropping old_yard_commit from upstream's history.
    git(&fixture.yard, &["reset", "--hard", &fixture.base_commit]);
    write_file(
        &fixture.yard.join("crates/engine/src/lib.rs"),
        "pub fn version() -> &'static str {\n    \"2-rewritten\"\n}\n",
    );
    let rewritten_yard_commit = git_commit_all(&fixture.yard, "rewritten yard v2");
    let before = RepoState::capture(&fixture.dev);

    let mut second_runner = UpdateRunner::new();
    let failure = update(&fixture.dev, &args, &mut second_runner).unwrap_err();

    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert!(
        failure.reason.contains(&old_yard_commit[..7]),
        "refusal must name the previously merged YARD commit {old_yard_commit}: {}",
        failure.reason
    );
    assert!(
        failure.reason.contains(&rewritten_yard_commit[..7]),
        "refusal must name the new YARD commit {rewritten_yard_commit}: {}",
        failure.reason
    );
    assert_eq!(RepoState::capture(&fixture.dev), before);
}

#[test]
fn refuses_continue_without_merge_in_progress() {
    let fixture = UpdateFixture::standard();
    let before = RepoState::capture(&fixture.dev);

    let args = UpdateArgs {
        reference: "main".into(),
        remote: None,
        base: None,
        resume: true,
    };
    let mut runner = UpdateRunner::new();
    let failure = update(&fixture.dev, &args, &mut runner).unwrap_err();

    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert_eq!(RepoState::capture(&fixture.dev), before);
}

#[test]
fn failed_check_leaves_update_branch_checked_out() {
    let fixture = UpdateFixture::standard();
    let expected_branch = fixture.expected_branch();
    let mut runner = UpdateRunner::failing_check();

    let failure = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap_err();
    assert_eq!(failure.step, "verify");
    assert!(
        !failure.is_refusal(),
        "a failed check is not a refusal: {failure}"
    );
    assert_eq!(
        current_branch(&fixture.dev).as_deref(),
        Some(expected_branch.as_str())
    );
    assert!(!merge_in_progress(&fixture.dev));
    assert_eq!(
        commit_parents(&fixture.dev, "HEAD"),
        vec![fixture.dev_commit.clone(), fixture.yard_commit.clone()]
    );
    assert_eq!(rev_oid(&fixture.dev, "main"), fixture.dev_commit);
}

#[test]
fn failed_yard_test_leaves_update_branch_checked_out() {
    let fixture = UpdateFixture::standard();
    let expected_branch = fixture.expected_branch();
    let mut runner = UpdateRunner::failing_test();

    let failure = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap_err();
    assert_eq!(failure.step, "verify");
    assert!(
        !failure.is_refusal(),
        "a failed check is not a refusal: {failure}"
    );
    assert_eq!(
        current_branch(&fixture.dev).as_deref(),
        Some(expected_branch.as_str())
    );
    assert_eq!(rev_oid(&fixture.dev, "main"), fixture.dev_commit);
}

fn assert_rolled_back(fixture: &UpdateFixture) {
    assert!(!merge_in_progress(&fixture.dev));
    assert!(is_working_tree_clean(&fixture.dev));
    assert_eq!(rev_oid(&fixture.dev, "HEAD"), fixture.dev_commit);
    assert_eq!(rev_oid(&fixture.dev, "main"), fixture.dev_commit);
    assert_eq!(
        current_branch(&fixture.dev),
        Some(fixture.expected_branch())
    );
}

fn run_recovery(fixture: &UpdateFixture, reason: &str) {
    let commands: Vec<_> = reason
        .lines()
        .filter_map(|line| line.strip_prefix("  git "))
        .collect();
    assert!(!commands.is_empty(), "{reason}");
    for command in commands {
        git(
            &fixture.dev,
            &command.split_whitespace().collect::<Vec<_>>(),
        );
    }
    assert!(!merge_in_progress(&fixture.dev));
    assert!(is_working_tree_clean(&fixture.dev));
    assert_eq!(current_branch(&fixture.dev).as_deref(), Some("main"));
    assert_eq!(rev_oid(&fixture.dev, "main"), fixture.dev_commit);
    assert!(!local_branches(&fixture.dev).contains(&fixture.expected_branch()));
}

#[test]
fn failed_metadata_rolls_back_the_merge() {
    let fixture = UpdateFixture::standard();
    let mut runner = UpdateRunner::new();
    runner.fail_metadata = true;
    let failure = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap_err();
    assert_eq!(failure.step, "metadata", "{failure}");
    assert!(!failure.is_refusal());
    assert_rolled_back(&fixture);
    run_recovery(&fixture, &failure.reason);
}

#[test]
fn continue_lock_refusal_preserves_resolutions_and_allows_aborting() {
    let mut fixture = UpdateFixture::moved_pin();
    let doc = "docs/x.md";
    write_file(&fixture.dev.join(doc), "Developer docs.\n");
    fixture.dev_commit = git_commit_all(&fixture.dev, "edit developer docs");
    write_file(&fixture.yard.join(doc), "YARD docs.\n");
    fixture.yard_commit = git_commit_all(&fixture.yard, "edit YARD docs");

    let mut args = args_for(&fixture);
    let mut runner = UpdateRunner::new();
    let first = update(&fixture.dev, &args, &mut runner).unwrap();
    assert!(matches!(first, Updated::Conflicts { paths, .. } if paths == [doc]));

    write_file(&fixture.dev.join(doc), "Resolved docs.\n");
    git(&fixture.dev, &["add", "--", doc]);
    let lock = fixture.dev.join("Cargo.lock");
    let staged_lock = fs::read_to_string(&lock).unwrap();
    write_file(&lock, &format!("{staged_lock}\n# Staged before continue\n"));
    git(&fixture.dev, &["add", "--", "Cargo.lock"]);
    let index_before = git(&fixture.dev, &["ls-files", "--stage", "--", "Cargo.lock"]);
    let staged_lock = fs::read_to_string(&lock).unwrap();
    write_file(
        &lock,
        &format!("{staged_lock}\n[[package]]\nname = \"north-star\"\nversion = \"0.1.0\"\n"),
    );
    let lock_before = fs::read(&lock).unwrap();
    args.resume = true;

    let failure = update(&fixture.dev, &args, &mut runner).unwrap_err();
    assert_eq!(failure.step, "lock", "{failure}");
    assert!(failure.is_refusal());
    assert!(merge_in_progress(&fixture.dev));
    assert_eq!(git(&fixture.dev, &["show", ":docs/x.md"]), "Resolved docs.");
    assert_eq!(
        git(&fixture.dev, &["ls-files", "--stage", "--", "Cargo.lock"]),
        index_before
    );
    assert!(
        fs::read(&lock).unwrap() == lock_before,
        "working tree lock changed"
    );
    assert!(
        failure
            .reason
            .contains("Change the named requirement on main")
    );
    assert!(failure.reason.contains("conflict resolutions"));
    run_recovery(&fixture, &failure.reason);
}

#[test]
fn unprepared_merge_reports_working_recovery_commands() {
    let fixture = UpdateFixture::standard();
    let branch = fixture.expected_branch();
    git(&fixture.dev, &["fetch", &fixture.yard_remote(), "main"]);
    git(&fixture.dev, &["checkout", "-b", &branch]);
    git(
        &fixture.dev,
        &["merge", "--no-commit", "--no-ff", "FETCH_HEAD"],
    );
    let lock = fixture.dev.join("Cargo.lock");
    let old = fs::read_to_string(&lock).unwrap();
    fs::write(&lock, format!("{old}\n# Unstaged change\n")).unwrap();

    let mut args = args_for(&fixture);
    args.resume = true;
    let failure = update(&fixture.dev, &args, &mut UpdateRunner::new()).unwrap_err();
    assert_eq!(failure.step, "arguments", "{failure}");
    assert!(merge_in_progress(&fixture.dev));
    run_recovery(&fixture, &failure.reason);
}

#[test]
fn refuses_base_before_changing_the_repository() {
    let fixture = UpdateFixture::standard();
    let before = RepoState::capture(&fixture.dev);
    let mut args = args_for(&fixture);
    args.base = Some(fixture.dev_commit.clone());
    let mut runner = UpdateRunner::new();
    let failure = update(&fixture.dev, &args, &mut runner).unwrap_err();
    assert_eq!(failure.step, "arguments", "{failure}");
    assert!(failure.reason.contains("--base"));
    assert!(failure.reason.contains("GitHub template"));
    assert_eq!(RepoState::capture(&fixture.dev), before);
    assert!(!runner.ran_git("fetch"));
}

#[test]
fn failed_resume_metadata_can_be_retried() {
    let (fixture, args) = staged_document_resolution();
    let before = RepoState::capture(&fixture.dev);
    let mut runner = UpdateRunner::new();
    runner.fail_metadata = true;
    let failure = update(&fixture.dev, &args, &mut runner).unwrap_err();
    assert_eq!(failure.step, "metadata");
    assert!(
        failure.reason.contains("cargo xtask update --continue"),
        "{failure}"
    );
    assert!(!failure.reason.contains("git merge --abort"), "{failure}");
    assert_eq!(RepoState::capture(&fixture.dev), before);
    assert_eq!(git(&fixture.dev, &["show", ":docs/x.md"]), "Resolved docs.");
    let resumed = update(&fixture.dev, &args, &mut UpdateRunner::new()).unwrap();
    assert!(matches!(resumed, Updated::Done { .. }), "{resumed:?}");
    assert_eq!(
        git(&fixture.dev, &["show", "HEAD:docs/x.md"]),
        "Resolved docs."
    );
}

#[test]
fn manually_committed_update_reports_working_recovery() {
    let (fixture, args) = staged_document_resolution();
    git(&fixture.dev, &["commit", "--no-edit"]);
    let failure = update(&fixture.dev, &args, &mut UpdateRunner::new()).unwrap_err();
    assert_eq!(failure.step, "arguments");
    assert!(failure.reason.contains("resolutions"), "{failure}");
    run_recovery(&fixture, &failure.reason);
}

fn staged_document_resolution() -> (UpdateFixture, UpdateArgs) {
    let mut fixture = UpdateFixture::standard();
    let doc = "docs/x.md";
    write_file(&fixture.dev.join(doc), "Developer docs.\n");
    fixture.dev_commit = git_commit_all(&fixture.dev, "edit developer docs");
    write_file(&fixture.yard.join(doc), "YARD docs.\n");
    fixture.yard_commit = git_commit_all(&fixture.yard, "edit YARD docs");
    let mut args = args_for(&fixture);
    let first = update(&fixture.dev, &args, &mut UpdateRunner::new()).unwrap();
    assert!(matches!(first, Updated::Conflicts { paths, .. } if paths == [doc]));
    write_file(&fixture.dev.join(doc), "Resolved docs.\n");
    git(&fixture.dev, &["add", "--", doc]);
    args.resume = true;
    (fixture, args)
}

#[test]
fn failed_checkout_does_not_add_upstream_remote() {
    let fixture = UpdateFixture::standard();
    git(&fixture.dev, &["branch", "yard-update"]);
    let failure = update(&fixture.dev, &args_for(&fixture), &mut UpdateRunner::new()).unwrap_err();
    assert_eq!(failure.step, "git");
    assert!(failure.reason.contains("git checkout"), "{failure}");
    assert!(
        git(&fixture.dev, &["remote"])
            .lines()
            .all(|remote| remote != "yard-upstream")
    );
    assert_eq!(rev_oid(&fixture.dev, "HEAD"), fixture.dev_commit);
}

#[test]
fn failed_check_keeps_upstream_remote_after_committing() {
    let fixture = UpdateFixture::standard();
    let failure = update(
        &fixture.dev,
        &args_for(&fixture),
        &mut UpdateRunner::failing_check(),
    )
    .unwrap_err();
    assert_eq!(failure.step, "verify");
    assert_eq!(
        git(&fixture.dev, &["remote", "get-url", "yard-upstream"]),
        fixture.yard_remote()
    );
}

#[test]
fn resume_accepts_developer_exclusion_resolution() {
    let mut fixture = UpdateFixture::at_base();
    let path = fixture.yard.join("Cargo.toml");
    let upstream = fs::read_to_string(&path).unwrap().replace(
        "[workspace.package]",
        "exclude = [\"app/north-star/extensions/notes\"]\n\n[workspace.package]",
    );
    write_file(&path, &upstream);
    fixture.yard_commit = git_commit_all(&fixture.yard, "exclude the application extension");
    let original = fs::read_to_string(fixture.dev.join("Cargo.toml")).unwrap();
    let mut args = args_for(&fixture);
    let mut runner = UpdateRunner::new();
    let first = update(&fixture.dev, &args, &mut runner).unwrap();
    assert!(
        matches!(&first, Updated::Conflicts { manifest_keys, .. }
            if manifest_keys.iter().any(|key| key.starts_with("workspace.exclude = "))),
        "{first:?}"
    );
    assert_eq!(
        fs::read_to_string(fixture.dev.join("Cargo.toml")).unwrap(),
        original
    );

    git(&fixture.dev, &["add", "Cargo.toml"]);
    args.resume = true;
    let resumed = update(&fixture.dev, &args, &mut runner).unwrap();
    assert!(matches!(resumed, Updated::Done { .. }), "{resumed:?}");
    assert_eq!(
        git(&fixture.dev, &["show", "HEAD:Cargo.toml"]),
        original.trim_end()
    );
}

#[test]
fn yard_renames_of_developer_edited_root_files_conflict() {
    let mut fixture = UpdateFixture::at_base();
    for name in ["LICENSE", "CLAUDE.md"] {
        write_file(&fixture.yard.join(name), "Original YARD file.\n");
    }
    let base = git_commit_all(&fixture.yard, "add YARD root files");
    git(&fixture.dev, &["fetch", &fixture.yard_remote(), "main"]);
    git(
        &fixture.dev,
        &["rebase", "--onto", &base, &fixture.base_commit],
    );
    for (name, renamed) in [("LICENSE", "LICENSE.md"), ("CLAUDE.md", "AI.md")] {
        write_file(&fixture.dev.join(name), "Developer edits.\n");
        git(&fixture.yard, &["mv", name, renamed]);
    }
    git_commit_all(&fixture.dev, "edit YARD root files");
    fixture.yard_commit = git_commit_all(&fixture.yard, "rename YARD root files");
    let outcome = update(&fixture.dev, &args_for(&fixture), &mut UpdateRunner::new()).unwrap();
    let Updated::Conflicts { paths, .. } = outcome else {
        panic!("expected both YARD root file renames to conflict, got {outcome:?}");
    };
    assert_eq!(paths, ["CLAUDE.md", "LICENSE"]);
}
