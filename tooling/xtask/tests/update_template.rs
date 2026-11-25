#![cfg(feature = "dev")]
//! Acceptance tests for part 2 of `upstream-updates`: `cargo xtask update` on a
//! repository made from the GitHub template.

#[allow(dead_code)]
mod support;

use std::fs;
use std::path::Path;

use support::tempdir;
use support::update::{
    RepoState, UpdateFixture, UpdateRunner, add_north_star, commit_message, commit_parents,
    current_branch, git, git_commit_all, git_init, is_ancestor, is_working_tree_clean,
    local_branches, lock_packages, merge_in_progress, rev_oid, root_commits, snapshot_dir,
    tree_oid, write_file, write_yard_v1_files,
};
use xtask::{UpdateArgs, Updated, update};

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
fn first_update_on_template_joins_and_merges() {
    let fixture = UpdateFixture::from_template();
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
                "expected lock changes when YARD updated engine and added helper"
            );
        }
        other => panic!("expected Updated::Done, got {other:?}"),
    }

    assert!(local_branches(&fixture.dev).contains(&expected_branch));
    assert_eq!(
        current_branch(&fixture.dev).as_deref(),
        Some(expected_branch.as_str())
    );
    assert!(!merge_in_progress(&fixture.dev));
    assert!(is_working_tree_clean(&fixture.dev));

    // Merge commit on the update branch has [join_commit, yard_commit].
    let merge_parents = commit_parents(&fixture.dev, "HEAD");
    assert_eq!(merge_parents.len(), 2, "expected a merge commit at HEAD");
    assert_eq!(merge_parents[1], fixture.yard_commit);
    assert!(
        commit_message(&fixture.dev, "HEAD").contains(&fixture.yard_commit),
        "merge commit message must contain the full YARD commit"
    );

    // Join commit has [dev_commit, base_commit], holds dev's tree unchanged, and changes no file.
    let join_commit = &merge_parents[0];
    assert_eq!(
        commit_parents(&fixture.dev, join_commit),
        vec![fixture.dev_commit.clone(), fixture.base_commit.clone()],
        "join commit must have developer HEAD and YARD base as its parents"
    );
    assert_eq!(
        tree_oid(&fixture.dev, join_commit),
        tree_oid(&fixture.dev, &fixture.dev_commit),
        "join commit must keep the developer's tree unchanged"
    );
    assert!(
        git(
            &fixture.dev,
            &[
                "diff-tree",
                "--no-commit-id",
                "--name-only",
                "-r",
                &fixture.dev_commit,
                join_commit,
            ],
        )
        .is_empty(),
        "join commit must change no file"
    );

    // YARD's new engine and helper changes arrived.
    assert_eq!(
        fs::read_to_string(fixture.dev.join("crates/engine/src/lib.rs")).unwrap(),
        fs::read_to_string(fixture.yard.join("crates/engine/src/lib.rs")).unwrap()
    );
    assert_eq!(
        fs::read_to_string(fixture.dev.join("crates/helper/src/lib.rs")).unwrap(),
        fs::read_to_string(fixture.yard.join("crates/helper/src/lib.rs")).unwrap()
    );

    // Developer's application folder and original branch are untouched.
    assert_eq!(
        snapshot_dir(&fixture.dev.join("app/north-star"), false),
        north_star_before,
        "app/north-star/ must be byte-identical after update"
    );
    assert_eq!(rev_oid(&fixture.dev, "main"), fixture.dev_commit);
    assert_eq!(tree_oid(&fixture.dev, "main"), main_tree_before);

    // Root Cargo.toml and Cargo.lock follow ownership rules.
    let merged_manifest = manifest_table(&fixture.dev);
    assert_eq!(
        string_array(&merged_manifest, "workspace", "default-members"),
        vec!["app/north-star"]
    );
    assert_eq!(
        string_array(&merged_manifest, "workspace", "members"),
        vec!["app/*", "crates/*", "app/north-star/extensions/*"]
    );
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
            .any(|(name, version, _)| name == "north-star" && version == "0.1.0")
    );

    assert!(runner.ran_cargo_metadata());
    assert!(runner.ran_cargo_test_yard());
    assert!(runner.ran_cargo_check_locked());
    assert!(!runner.ran_git("push"));
}

#[test]
fn picks_newest_matching_yard_ancestor_when_consecutive_commits_share_tree() {
    let mut fixture = UpdateFixture::from_template_at_base();
    git(
        &fixture.yard,
        &[
            "commit",
            "--allow-empty",
            "-m",
            "empty commit with same tree",
        ],
    );
    let newer_base = rev_oid(&fixture.yard, "HEAD");
    assert_ne!(newer_base, fixture.base_commit);
    assert_eq!(
        tree_oid(&fixture.yard, &newer_base),
        tree_oid(&fixture.yard, &fixture.base_commit)
    );
    fixture.advance_yard_standard();

    let mut runner = UpdateRunner::new();
    let outcome = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap();
    assert!(matches!(outcome, Updated::Done { .. }), "{outcome:?}");

    let merge_parents = commit_parents(&fixture.dev, "HEAD");
    assert_eq!(merge_parents.len(), 2);
    assert_eq!(merge_parents[1], fixture.yard_commit);
    let join_commit = &merge_parents[0];
    assert_eq!(
        commit_parents(&fixture.dev, join_commit),
        vec![fixture.dev_commit.clone(), newer_base],
        "expected the newer YARD commit with the matching tree as the base"
    );
}

#[test]
fn second_update_after_merging_first_is_ordinary() {
    let fixture = UpdateFixture::from_template();
    let args = args_for(&fixture);

    let Updated::Done { branch, .. } =
        update(&fixture.dev, &args, &mut UpdateRunner::new()).unwrap()
    else {
        panic!("expected Updated::Done on first update");
    };
    git(&fixture.dev, &["checkout", "main"]);
    git(&fixture.dev, &["merge", &branch]);
    let main_tip = rev_oid(&fixture.dev, "main");
    let north_star_before = snapshot_dir(&fixture.dev.join("app/north-star"), false);

    // Advance YARD to v3.
    write_file(
        &fixture.yard.join("crates/engine/src/lib.rs"),
        "pub fn version() -> &'static str {\n    \"3\"\n}\n",
    );
    let yard_v3 = git_commit_all(&fixture.yard, "yard v3");
    let expected_second_branch = format!("yard-update/{}", &yard_v3[..12]);

    let mut runner = UpdateRunner::new();
    let outcome = update(&fixture.dev, &args, &mut runner).unwrap();
    match outcome {
        Updated::Done {
            branch,
            yard_commit,
            start,
            ..
        } => {
            assert_eq!(branch, expected_second_branch);
            assert_eq!(yard_commit, yard_v3);
            assert_eq!(start, "main");
        }
        other => panic!("expected Updated::Done on second update, got {other:?}"),
    }

    // Ordinary merge: no join commit, merge commit's first parent is main's tip directly.
    assert_eq!(
        commit_parents(&fixture.dev, "HEAD"),
        vec![main_tip.clone(), yard_v3.clone()],
        "second update must make no join commit"
    );
    assert_eq!(rev_oid(&fixture.dev, "main"), main_tip);
    assert_eq!(
        fs::read_to_string(fixture.dev.join("crates/engine/src/lib.rs")).unwrap(),
        "pub fn version() -> &'static str {\n    \"3\"\n}\n"
    );
    assert_eq!(
        snapshot_dir(&fixture.dev.join("app/north-star"), false),
        north_star_before
    );
    assert!(runner.ran_cargo_test_yard());
    assert!(runner.ran_cargo_check_locked());
}

#[test]
fn refuses_template_when_no_yard_ancestor_matches_root_tree() {
    let fixture = UpdateFixture::from_template_with_mismatched_root();
    let before = RepoState::capture(&fixture.dev);

    let mut runner = UpdateRunner::new();
    let failure = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap_err();

    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert!(
        failure.reason.contains("--base"),
        "refusal must name --base: {}",
        failure.reason
    );
    assert_eq!(RepoState::capture(&fixture.dev), before);
    assert!(!runner.ran_git("merge"));
}

#[test]
fn refuses_template_with_more_than_one_root_commit() {
    let fixture = UpdateFixture::from_template();
    git(&fixture.dev, &["checkout", "--orphan", "extra-root"]);
    git(&fixture.dev, &["rm", "-rf", "--quiet", "."]);
    write_file(&fixture.dev.join("EXTRA.md"), "second root\n");
    git_commit_all(&fixture.dev, "second root commit");
    git(&fixture.dev, &["checkout", "main"]);
    git(
        &fixture.dev,
        &[
            "merge",
            "--allow-unrelated-histories",
            "--no-edit",
            "extra-root",
        ],
    );
    assert_eq!(root_commits(&fixture.dev).len(), 2);

    let before = RepoState::capture(&fixture.dev);
    let mut runner = UpdateRunner::new();
    let failure = update(&fixture.dev, &args_for(&fixture), &mut runner).unwrap_err();

    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert!(
        failure.reason.contains("--base"),
        "refusal must name --base: {}",
        failure.reason
    );
    assert_eq!(RepoState::capture(&fixture.dev), before);
    assert!(!runner.ran_git("merge"));
}

#[test]
fn refuses_template_when_matching_yard_commits_are_not_on_one_line() {
    let temp = tempdir();
    let yard = temp.path().join("yard");
    let dev = temp.path().join("dev");

    // YARD starts at r0, then two branches (branch-a on main, branch-b) both
    // reach identical tree T independently before merging and advancing.
    git_init(&yard);
    write_yard_v1_files(&yard);
    write_file(&yard.join("docs/x.md"), "v0\n");
    let r0 = git_commit_all(&yard, "r0");

    write_file(
        &yard.join("docs/x.md"),
        "# YARD Docs\n\nLine one.\nLine two.\nLine three.\n",
    );
    let commit_a = git_commit_all(&yard, "branch a matching tree");

    write_file(
        &yard.join("crates/engine/src/lib.rs"),
        "pub fn version() -> &'static str {\n    \"1.5\"\n}\n",
    );
    git_commit_all(&yard, "advance main past branch a");

    git(&yard, &["checkout", "-b", "branch-b", &r0]);
    write_file(
        &yard.join("docs/x.md"),
        "# YARD Docs\n\nLine one.\nLine two.\nLine three.\n",
    );
    let commit_b = git_commit_all(&yard, "branch b matching tree");
    assert_eq!(tree_oid(&yard, &commit_a), tree_oid(&yard, &commit_b));
    assert!(!is_ancestor(&yard, &commit_a, &commit_b));
    assert!(!is_ancestor(&yard, &commit_b, &commit_a));

    git(&yard, &["checkout", "main"]);
    git(&yard, &["merge", "--no-edit", "branch-b"]);
    assert_ne!(tree_oid(&yard, "HEAD"), tree_oid(&yard, &commit_a));
    write_file(
        &yard.join("crates/engine/src/lib.rs"),
        "pub fn version() -> &'static str {\n    \"2\"\n}\n",
    );
    git_commit_all(&yard, "yard v2");

    // Template repository whose root commit has tree T (commit_a / commit_b).
    git_init(&dev);
    git(&yard, &["checkout", "--detach", &commit_a]);
    let prefix = format!("--prefix={}/", dev.to_string_lossy().replace('\\', "/"));
    git(&yard, &["checkout-index", "--all", &prefix]);
    git(&yard, &["checkout", "main"]);
    let dev_root = git_commit_all(&dev, "template root");
    assert_eq!(tree_oid(&dev, &dev_root), tree_oid(&yard, &commit_a));
    add_north_star(&dev);

    let before = RepoState::capture(&dev);
    let args = UpdateArgs {
        reference: "main".into(),
        remote: Some(yard.to_string_lossy().into_owned()),
        base: None,
        resume: false,
    };
    let mut runner = UpdateRunner::new();
    let failure = update(&dev, &args, &mut runner).unwrap_err();

    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert!(
        failure.reason.contains("--base"),
        "refusal must name --base: {}",
        failure.reason
    );
    assert_eq!(RepoState::capture(&dev), before);
    assert!(!runner.ran_git("merge"));
}

#[test]
fn conflicts_on_template_resume_with_continue_and_keep_join_commit() {
    let mut fixture = UpdateFixture::from_template_at_base();
    write_file(
        &fixture.dev.join("docs/x.md"),
        "# YARD Docs\n\nLine one from developer.\nLine two.\nLine three.\n",
    );
    let dev_head = git_commit_all(&fixture.dev, "edit docs/x.md on developer side");
    let north_star_before = snapshot_dir(&fixture.dev.join("app/north-star"), false);

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

    assert_eq!(
        current_branch(&fixture.dev).as_deref(),
        Some(expected_branch.as_str())
    );
    assert!(merge_in_progress(&fixture.dev));
    assert_eq!(rev_oid(&fixture.dev, "main"), dev_head);
    assert!(!first_runner.ran_cargo_test_yard());
    assert!(!first_runner.ran_cargo_check_locked());

    let resolved = "# YARD Docs\n\nLine one merged.\nLine two.\nLine three.\n";
    write_file(&fixture.dev.join("docs/x.md"), resolved);
    git(&fixture.dev, &["add", "docs/x.md"]);

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

    let merge_parents = commit_parents(&fixture.dev, "HEAD");
    assert_eq!(merge_parents.len(), 2);
    assert_eq!(merge_parents[1], fixture.yard_commit);
    let join_commit = &merge_parents[0];
    assert_eq!(
        commit_parents(&fixture.dev, join_commit),
        vec![dev_head.clone(), fixture.base_commit.clone()],
        "join commit must remain in history after --continue"
    );
    assert_eq!(
        tree_oid(&fixture.dev, join_commit),
        tree_oid(&fixture.dev, &dev_head)
    );

    assert_eq!(rev_oid(&fixture.dev, "main"), dev_head);
    assert_eq!(
        fs::read_to_string(fixture.dev.join("docs/x.md")).unwrap(),
        resolved
    );
    assert_eq!(
        snapshot_dir(&fixture.dev.join("app/north-star"), false),
        north_star_before
    );
    assert!(resume_runner.ran_cargo_test_yard());
    assert!(resume_runner.ran_cargo_check_locked());
    assert!(!resume_runner.ran_git("push"));
}

#[test]
fn refuses_base_on_repository_sharing_history_with_yard() {
    let fixture = UpdateFixture::standard();
    let before = RepoState::capture(&fixture.dev);
    let mut args = args_for(&fixture);
    args.base = Some(fixture.base_commit.clone());

    let mut runner = UpdateRunner::new();
    let failure = update(&fixture.dev, &args, &mut runner).unwrap_err();

    assert_eq!(failure.step, "arguments");
    assert!(failure.is_refusal());
    assert!(
        failure.reason.contains("--base"),
        "refusal must name --base: {}",
        failure.reason
    );
    assert!(
        failure.reason.to_ascii_lowercase().contains("template"),
        "refusal must say --base is only for a repository made from the template: {}",
        failure.reason
    );
    assert_eq!(RepoState::capture(&fixture.dev), before);
    assert!(!runner.ran_git("merge"));
}

#[test]
fn with_base_joins_and_merges_template_with_multiple_roots() {
    let fixture = UpdateFixture::from_template();
    git(&fixture.dev, &["checkout", "--orphan", "extra-root"]);
    git(&fixture.dev, &["rm", "-rf", "--quiet", "."]);
    write_file(&fixture.dev.join("EXTRA.md"), "second root\n");
    git_commit_all(&fixture.dev, "second root commit");
    git(&fixture.dev, &["checkout", "main"]);
    git(
        &fixture.dev,
        &[
            "merge",
            "--allow-unrelated-histories",
            "--no-edit",
            "extra-root",
        ],
    );
    assert_eq!(root_commits(&fixture.dev).len(), 2);

    let dev_head = rev_oid(&fixture.dev, "main");
    let main_tree_before = tree_oid(&fixture.dev, "main");
    let north_star_before = snapshot_dir(&fixture.dev.join("app/north-star"), false);
    let expected_branch = fixture.expected_branch();

    let mut args = args_for(&fixture);
    args.base = Some(fixture.base_commit.clone());
    let mut runner = UpdateRunner::new();
    let outcome = update(&fixture.dev, &args, &mut runner).unwrap();

    match outcome {
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
        other => panic!("expected Updated::Done, got {other:?}"),
    }

    let merge_parents = commit_parents(&fixture.dev, "HEAD");
    assert_eq!(merge_parents.len(), 2);
    assert_eq!(merge_parents[1], fixture.yard_commit);
    let join_commit = &merge_parents[0];
    assert_eq!(
        commit_parents(&fixture.dev, join_commit),
        vec![dev_head.clone(), fixture.base_commit.clone()],
        "join commit must have dev HEAD and --base as its parents"
    );
    assert_eq!(
        tree_oid(&fixture.dev, join_commit),
        tree_oid(&fixture.dev, &dev_head)
    );
    assert_eq!(rev_oid(&fixture.dev, "main"), dev_head);
    assert_eq!(tree_oid(&fixture.dev, "main"), main_tree_before);
    assert_eq!(
        snapshot_dir(&fixture.dev.join("app/north-star"), false),
        north_star_before
    );
    assert_eq!(
        fs::read_to_string(fixture.dev.join("EXTRA.md")).unwrap(),
        "second root\n"
    );
    assert!(runner.ran_cargo_test_yard());
    assert!(runner.ran_cargo_check_locked());
    assert!(!runner.ran_git("push"));
}

#[test]
fn refuses_replaced_upstream_after_template_update_was_merged() {
    let fixture = UpdateFixture::from_template();
    let Updated::Done { branch, .. } =
        update(&fixture.dev, &args_for(&fixture), &mut UpdateRunner::new()).unwrap()
    else {
        panic!("expected Updated::Done on first update");
    };
    git(&fixture.dev, &["checkout", "main"]);
    git(&fixture.dev, &["merge", &branch]);

    // Replace YARD's main with an unrelated history.
    git(&fixture.yard, &["checkout", "--orphan", "replacement"]);
    write_file(
        &fixture.yard.join("crates/engine/src/lib.rs"),
        "pub fn version() -> &'static str {\n    \"replaced\"\n}\n",
    );
    let new_tip = git_commit_all(&fixture.yard, "replaced yard");
    git(&fixture.yard, &["branch", "-f", "main", "replacement"]);
    git(&fixture.yard, &["checkout", "main"]);
    assert!(!is_ancestor(&fixture.yard, &fixture.yard_commit, &new_tip));

    for base in [None, Some(new_tip.clone())] {
        let before = RepoState::capture(&fixture.dev);
        let mut args = args_for(&fixture);
        args.base = base.clone();
        let mut runner = UpdateRunner::new();
        let failure = update(&fixture.dev, &args, &mut runner).unwrap_err();

        assert_eq!(failure.step, "arguments", "--base {base:?}: {failure}");
        assert!(failure.is_refusal(), "--base {base:?}: {failure}");
        assert!(
            failure.reason.contains("rewritten")
                && failure.reason.contains(&new_tip)
                && failure.reason.contains(&fixture.yard_commit),
            "refusal must name the rewrite and both commits (--base {base:?}): {}",
            failure.reason
        );
        assert_eq!(RepoState::capture(&fixture.dev), before);
        assert!(!runner.ran_git("merge"));
    }
}

#[test]
fn refuses_base_with_continue_keeping_the_merge_and_resolution() {
    let mut fixture = UpdateFixture::from_template_at_base();
    write_file(
        &fixture.dev.join("docs/x.md"),
        "# YARD Docs\n\nLine one from developer.\nLine two.\nLine three.\n",
    );
    git_commit_all(&fixture.dev, "edit docs/x.md on developer side");
    write_file(
        &fixture.yard.join("docs/x.md"),
        "# YARD Docs\n\nLine one from YARD.\nLine two.\nLine three.\n",
    );
    fixture.advance_yard_standard();
    let first = update(&fixture.dev, &args_for(&fixture), &mut UpdateRunner::new()).unwrap();
    assert!(matches!(first, Updated::Conflicts { .. }), "{first:?}");

    let resolved = "# YARD Docs\n\nLine one merged.\nLine two.\nLine three.\n";
    write_file(&fixture.dev.join("docs/x.md"), resolved);
    git(&fixture.dev, &["add", "docs/x.md"]);
    let before = RepoState::capture(&fixture.dev);

    let args = UpdateArgs {
        reference: "main".into(),
        remote: None,
        base: Some(fixture.base_commit.clone()),
        resume: true,
    };
    let mut runner = UpdateRunner::new();
    let failure = update(&fixture.dev, &args, &mut runner).unwrap_err();

    assert_eq!(failure.step, "arguments", "{failure}");
    assert!(failure.is_refusal());
    assert!(
        failure.reason.contains("--base") && failure.reason.contains("first update"),
        "refusal must name --base and say it applies to the first update: {}",
        failure.reason
    );
    assert_eq!(RepoState::capture(&fixture.dev), before);
    assert!(merge_in_progress(&fixture.dev));
    assert_eq!(
        git(&fixture.dev, &["show", ":docs/x.md"]),
        resolved.trim_end()
    );
    assert!(!runner.ran_git("commit"));
    assert!(!runner.ran_cargo_test_yard());
}
