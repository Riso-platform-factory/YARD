//! Test runner and git/Cargo workspace fixtures for `cargo xtask update`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use super::{Recorded, TempDir, tempdir};
use xtask::{Captured, Interrupt, Runner, SystemRunner};

const AUTHOR_NAME: &str = "Test Developer";
const AUTHOR_EMAIL: &str = "test@example.invalid";

/// Extracts the normalized program name (file name without `.exe`).
fn program_name(command: &Command) -> String {
    let name = Path::new(command.get_program())
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    match name.strip_suffix(".exe") {
        Some(stem) => stem.to_owned(),
        None => name,
    }
}

/// Captures a command's program, arguments, environment overrides, working
/// directory, and optional stdin into a [`Recorded`] entry.
fn record_command(command: &Command, input: Option<&[u8]>) -> Recorded {
    Recorded {
        program: program_name(command),
        path: PathBuf::from(command.get_program()),
        args: command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect(),
        envs: command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect(),
        dir: command.get_current_dir().map(Path::to_path_buf),
        input: input.map(|bytes| String::from_utf8_lossy(bytes).into_owned()),
    }
}

/// Returns the `git` subcommand from a git argument list, skipping leading
/// global flags (such as `-C <dir>` or `-c <k=v>`).
pub fn git_subcommand(args: &[String]) -> Option<&str> {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if matches!(
            arg,
            "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace" | "--super-prefix"
        ) {
            index += 2;
        } else if arg.starts_with('-') {
            index += 1;
        } else {
            return Some(arg);
        }
    }
    None
}

/// Returns the `cargo` subcommand from a cargo argument list, skipping leading
/// toolchain selectors or global flags.
pub fn cargo_subcommand(args: &[String]) -> Option<&str> {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg.starts_with('+') {
            index += 1;
        } else if matches!(arg, "-C" | "-Z" | "--config" | "--color") {
            index += 2;
        } else if arg.starts_with('-') {
            index += 1;
        } else {
            return Some(arg);
        }
    }
    None
}

/// A runner for `xtask::update` tests: runs `git` and `cargo metadata` (and
/// any other real cargo helper command) through [`xtask::SystemRunner`],
/// records every command, and intercepts `cargo test` and `cargo check` so
/// they succeed (or fail when configured) without building the workspace.
pub struct UpdateRunner {
    pub commands: Vec<Recorded>,
    pub fail_test: bool,
    pub fail_check: bool,
    pub fail_metadata: bool,
    system: SystemRunner,
}

impl Default for UpdateRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdateRunner {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            fail_test: false,
            fail_check: false,
            fail_metadata: false,
            system: SystemRunner {
                interrupt: Interrupt::default(),
            },
        }
    }

    pub fn failing_check() -> Self {
        Self {
            fail_check: true,
            ..Self::new()
        }
    }

    pub fn failing_test() -> Self {
        Self {
            fail_test: true,
            ..Self::new()
        }
    }

    /// Returns `Some(true)` if the command is an intercepted check that should
    /// succeed, `Some(false)` if it should fail, or `None` if it should run for
    /// real.
    fn intercepted_check(&self, recorded: &Recorded) -> Option<(&'static str, bool)> {
        if recorded.program != "cargo" {
            return None;
        }
        match cargo_subcommand(&recorded.args) {
            Some("test") => Some(("test", !self.fail_test)),
            Some("metadata") if self.fail_metadata => Some(("metadata", false)),
            Some("check") => Some(("check", !self.fail_check)),
            _ => None,
        }
    }

    /// Whether a recorded `git` command invoked `subcommand`.
    pub fn ran_git(&self, subcommand: &str) -> bool {
        self.commands
            .iter()
            .any(|cmd| cmd.program == "git" && git_subcommand(&cmd.args) == Some(subcommand))
    }

    /// Whether `cargo test -p yard` (or `--package yard`) was recorded.
    pub fn ran_cargo_test_yard(&self) -> bool {
        self.commands.iter().any(|cmd| {
            if cmd.program != "cargo" || cargo_subcommand(&cmd.args) != Some("test") {
                return false;
            }
            cmd.args
                .windows(2)
                .any(|pair| matches!(pair[0].as_str(), "-p" | "--package") && pair[1] == "yard")
                || cmd.args.iter().any(|arg| arg == "--package=yard")
        })
    }

    /// Whether `cargo check --workspace --all-targets --locked` was recorded.
    pub fn ran_cargo_check_locked(&self) -> bool {
        self.commands.iter().any(|cmd| {
            cmd.program == "cargo"
                && cargo_subcommand(&cmd.args) == Some("check")
                && cmd.args.iter().any(|arg| arg == "--workspace")
                && cmd.args.iter().any(|arg| arg == "--all-targets")
                && cmd.args.iter().any(|arg| arg == "--locked")
        })
    }

    /// Whether `cargo metadata --format-version 1` was recorded.
    pub fn ran_cargo_metadata(&self) -> bool {
        self.commands.iter().any(|cmd| {
            cmd.program == "cargo"
                && cargo_subcommand(&cmd.args) == Some("metadata")
                && (cmd
                    .args
                    .windows(2)
                    .any(|pair| pair[0] == "--format-version" && pair[1] == "1")
                    || cmd.args.iter().any(|arg| arg == "--format-version=1"))
        })
    }
}

impl Runner for UpdateRunner {
    fn run(&mut self, command: &mut Command) -> Result<(), String> {
        let recorded = record_command(command, None);
        self.commands.push(recorded.clone());
        if let Some((sub, ok)) = self.intercepted_check(&recorded) {
            return if ok {
                Ok(())
            } else {
                Err(format!("cargo {sub} exited with exit status: 1"))
            };
        }
        self.system.run(command)
    }

    fn output(&mut self, command: &mut Command) -> Result<Vec<u8>, String> {
        let recorded = record_command(command, None);
        self.commands.push(recorded.clone());
        if let Some((sub, ok)) = self.intercepted_check(&recorded) {
            return if ok {
                Ok(Vec::new())
            } else {
                Err(format!("cargo {sub} exited with exit status: 1"))
            };
        }
        self.system.output(command)
    }

    fn report(&mut self, command: &mut Command) -> Result<(bool, Vec<u8>), String> {
        let recorded = record_command(command, None);
        self.commands.push(recorded.clone());
        if let Some((sub, ok)) = self.intercepted_check(&recorded) {
            return if ok {
                Ok((true, Vec::new()))
            } else {
                Ok((false, format!("cargo {sub} failed\n").into_bytes()))
            };
        }
        self.system.report(command)
    }

    fn input(&mut self, command: &mut Command, input: &[u8]) -> Result<Vec<u8>, String> {
        let recorded = record_command(command, Some(input));
        self.commands.push(recorded.clone());
        if let Some((sub, ok)) = self.intercepted_check(&recorded) {
            return if ok {
                Ok(Vec::new())
            } else {
                Err(format!("cargo {sub} exited with exit status: 1"))
            };
        }
        self.system.input(command, input)
    }

    fn capture(&mut self, command: &mut Command) -> Result<Captured, String> {
        let recorded = record_command(command, None);
        self.commands.push(recorded.clone());
        if let Some((sub, ok)) = self.intercepted_check(&recorded) {
            return Ok(if ok {
                Captured {
                    success: true,
                    status: "exit status: 0".into(),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                }
            } else {
                Captured {
                    success: false,
                    status: "exit status: 1".into(),
                    stdout: Vec::new(),
                    stderr: format!("cargo {sub} failed\n").into_bytes(),
                }
            });
        }
        self.system.capture(command)
    }
}

/// Builds a `git` command configured with deterministic test author/committer
/// identity and `protocol.file.allow=always`.
pub fn git_cmd(dir: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir)
        .args(["-c", "protocol.file.allow=always"])
        .env("GIT_AUTHOR_NAME", AUTHOR_NAME)
        .env("GIT_AUTHOR_EMAIL", AUTHOR_EMAIL)
        .env("GIT_COMMITTER_NAME", AUTHOR_NAME)
        .env("GIT_COMMITTER_EMAIL", AUTHOR_EMAIL);
    cmd
}

/// Runs `git` in `dir`, asserting success, and returns trimmed stdout.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let output = git_cmd(dir)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("failed to spawn git {args:?} in {}: {err}", dir.display()));
    assert!(
        output.status.success(),
        "git {args:?} failed in {} ({}):\nstdout:\n{}\nstderr:\n{}",
        dir.display(),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// Runs `git` in `dir` and returns its exit status without asserting success.
pub fn git_status(dir: &Path, args: &[&str]) -> ExitStatus {
    git_cmd(dir)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("failed to spawn git {args:?} in {}: {err}", dir.display()))
        .status
}

/// Configures local test identity, disables GPG signing, and allows local
/// `file://` fetches in `dir`.
pub fn configure_git_repo(dir: &Path) {
    git(dir, &["config", "user.name", AUTHOR_NAME]);
    git(dir, &["config", "user.email", AUTHOR_EMAIL]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    git(dir, &["config", "protocol.file.allow", "always"]);
}

/// Initializes a new git repository with default branch `main`.
pub fn git_init(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    git(dir, &["-c", "init.defaultBranch=main", "init"]);
    configure_git_repo(dir);
}

/// Stages all files in `dir`, commits them with `message`, and returns `HEAD`.
pub fn git_commit_all(dir: &Path, message: &str) -> String {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-m", message]);
    head_oid(dir)
}

pub fn head_oid(dir: &Path) -> String {
    git(dir, &["rev-parse", "HEAD"])
}

pub fn rev_oid(dir: &Path, rev: &str) -> String {
    git(dir, &["rev-parse", rev])
}

pub fn is_ancestor(dir: &Path, older: &str, newer: &str) -> bool {
    git_status(dir, &["merge-base", "--is-ancestor", older, newer]).success()
}

pub fn root_commits(dir: &Path) -> Vec<String> {
    git(dir, &["rev-list", "--max-parents=0", "HEAD"])
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn tree_oid(dir: &Path, rev: &str) -> String {
    git(dir, &["rev-parse", &format!("{rev}^{{tree}}")])
}

/// Returns the checked-out branch name, or `None` when `HEAD` is detached.
pub fn current_branch(dir: &Path) -> Option<String> {
    let output = git_cmd(dir)
        .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
        .output()
        .unwrap();
    if output.status.success() {
        Some(String::from_utf8(output.stdout).unwrap().trim().to_owned())
    } else {
        None
    }
}

/// Sorted list of all local branch names (`refs/heads/*`).
pub fn local_branches(dir: &Path) -> Vec<String> {
    let stdout = git(
        dir,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    );
    let mut branches: Vec<String> = stdout
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    branches.sort();
    branches
}

/// Parent commit hashes of `rev`, in git's parent order.
pub fn commit_parents(dir: &Path, rev: &str) -> Vec<String> {
    let stdout = git(dir, &["rev-list", "--parents", "-n", "1", rev]);
    stdout
        .split_whitespace()
        .skip(1)
        .map(str::to_owned)
        .collect()
}

pub fn commit_message(dir: &Path, rev: &str) -> String {
    git(dir, &["log", "-1", "--format=%B", rev])
}

pub fn merge_in_progress(dir: &Path) -> bool {
    git_status(dir, &["rev-parse", "--verify", "-q", "MERGE_HEAD"]).success()
}

pub fn is_working_tree_clean(dir: &Path) -> bool {
    git(dir, &["status", "--porcelain"]).is_empty()
}

/// Formats a local filesystem path as a `file://` URL suitable for git and
/// Cargo on both Unix and Windows.
pub fn file_url(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    }
}

/// Generates `Cargo.lock` in `dir` via `cargo generate-lockfile`.
pub fn cargo_generate_lockfile(dir: &Path, offline: bool) {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(dir).arg("generate-lockfile");
    if offline {
        cmd.arg("--offline");
    }
    let output = cmd
        .output()
        .unwrap_or_else(|err| panic!("failed to run cargo generate-lockfile: {err}"));
    assert!(
        output.status.success(),
        "cargo generate-lockfile failed in {}:\nstdout:\n{}\nstderr:\n{}",
        dir.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Dir,
    File(Vec<u8>),
}

/// Captures every directory and file under `dir` with its exact bytes. When
/// `skip_git` is true, the top-level `.git` directory is skipped.
pub fn snapshot_dir(dir: &Path, skip_git: bool) -> BTreeMap<String, Entry> {
    fn walk(root: &Path, current: &Path, skip_git: bool, out: &mut BTreeMap<String, Entry>) {
        for entry in fs::read_dir(current).unwrap() {
            let entry = entry.unwrap();
            if skip_git && current == root && entry.file_name() == ".git" {
                continue;
            }
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if entry.file_type().unwrap().is_dir() {
                out.insert(relative, Entry::Dir);
                walk(root, &path, skip_git, out);
            } else {
                out.insert(relative, Entry::File(fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, skip_git, &mut out);
    out
}

/// Observable repository state used to prove a refusal changes nothing:
/// current branch (or detached), `HEAD` commit, local branches, merge state,
/// and working-tree files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoState {
    pub branch: Option<String>,
    pub head: String,
    pub branches: Vec<String>,
    pub merge_in_progress: bool,
    pub files: BTreeMap<String, Entry>,
}

impl RepoState {
    pub fn capture(root: &Path) -> Self {
        Self {
            branch: current_branch(root),
            head: head_oid(root),
            branches: local_branches(root),
            merge_in_progress: merge_in_progress(root),
            files: snapshot_dir(root, true),
        }
    }
}

/// Reads `(name, version, source)` tuples from a `Cargo.lock` file, sorted.
pub fn lock_packages(path: &Path) -> Vec<(String, String, Option<String>)> {
    let text = fs::read_to_string(path).unwrap();
    let table: toml::Table = toml::from_str(&text).unwrap();
    let mut packages: Vec<_> = table
        .get("package")
        .and_then(toml::Value::as_array)
        .unwrap_or_else(|| panic!("no [[package]] entries in {}", path.display()))
        .iter()
        .map(|pkg| {
            let name = pkg["name"].as_str().unwrap().to_owned();
            let version = pkg["version"].as_str().unwrap().to_owned();
            let source = pkg
                .get("source")
                .and_then(toml::Value::as_str)
                .map(str::to_owned);
            (name, version, source)
        })
        .collect();
    packages.sort();
    packages
}

pub fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

const YARD_V1_ROOT_MANIFEST: &str = "\
# YARD workspace manifest
[workspace]
resolver = \"3\"
members = [\"app/*\", \"crates/*\"]
default-members = [\"app/yard\"]

[workspace.package]
edition = \"2024\"
publish = false
license = \"GPL-3.0-or-later\"

[workspace.dependencies]
engine = { path = \"crates/engine\" }
";

const YARD_V2_ROOT_MANIFEST: &str = "\
# YARD workspace manifest v2
[workspace]
resolver = \"3\"
members = [\"app/*\", \"crates/*\"]
default-members = [\"app/yard\"]

[workspace.package]
edition = \"2024\"
publish = false
license = \"GPL-3.0-or-later\"

[workspace.dependencies]
engine = { path = \"crates/engine\" }
helper = { path = \"crates/helper\" }
";

const DEV_ROOT_MANIFEST: &str = "\
# YARD workspace manifest
[workspace]
resolver = \"3\"
members = [\"app/*\", \"crates/*\", \"app/north-star/extensions/*\"]
default-members = [\"app/north-star\"]

[workspace.package]
edition = \"2024\"
publish = false
license = \"GPL-3.0-or-later\"

[workspace.dependencies]
engine = { path = \"crates/engine\" }
";

/// A paired YARD upstream repository and developer clone under one temporary
/// directory, sharing history at `base_commit`.
pub struct UpdateFixture {
    temp: TempDir,
    pub yard: PathBuf,
    pub dev: PathBuf,
    pub base_commit: String,
    pub dev_commit: String,
    pub yard_commit: String,
}

pub fn write_yard_v1_files(yard: &Path) {
    write_file(&yard.join("Cargo.toml"), YARD_V1_ROOT_MANIFEST);
    write_file(
        &yard.join("app/yard/Cargo.toml"),
        "[package]\nname = \"yard\"\nversion = \"0.1.0\"\nedition.workspace = true\n\n\
         [dependencies]\nengine.workspace = true\n",
    );
    write_file(
        &yard.join("app/yard/src/main.rs"),
        "fn main() {\n    let _ = engine::version();\n}\n",
    );
    write_file(
        &yard.join("crates/engine/Cargo.toml"),
        "[package]\nname = \"engine\"\nversion = \"0.1.0\"\nedition.workspace = true\n",
    );
    write_file(
        &yard.join("crates/engine/src/lib.rs"),
        "pub fn version() -> &'static str {\n    \"1\"\n}\n",
    );
    write_file(
        &yard.join("docs/x.md"),
        "# YARD Docs\n\nLine one.\nLine two.\nLine three.\n",
    );
    cargo_generate_lockfile(yard, true);
}

fn create_yard_v1(yard: &Path) -> String {
    git_init(yard);
    write_yard_v1_files(yard);
    git_commit_all(yard, "initial yard")
}

pub fn add_north_star(dev: &Path) -> String {
    write_file(&dev.join("Cargo.toml"), DEV_ROOT_MANIFEST);
    write_file(
        &dev.join("app/north-star/Cargo.toml"),
        "[package]\nname = \"north-star\"\nversion = \"0.1.0\"\nedition.workspace = true\n\n\
         [dependencies]\nengine.workspace = true\nnotes = { path = \"extensions/notes\" }\n",
    );
    write_file(
        &dev.join("app/north-star/src/main.rs"),
        "fn main() {\n    let _ = (engine::version(), notes::name());\n}\n",
    );
    write_file(
        &dev.join("app/north-star/extensions/notes/Cargo.toml"),
        "[package]\nname = \"notes\"\nversion = \"0.1.0\"\nedition.workspace = true\n",
    );
    write_file(
        &dev.join("app/north-star/extensions/notes/src/lib.rs"),
        "pub fn name() -> &'static str {\n    \"notes\"\n}\n",
    );
    cargo_generate_lockfile(dev, true);
    git_commit_all(dev, "add north-star application")
}

impl UpdateFixture {
    /// Creates YARD at commit 1 (`base_commit`) and clones it to `dev`, adding
    /// `app/north-star/` and `app/north-star/extensions/notes/` (`dev_commit`).
    /// YARD is still at `base_commit` so callers can customize commit 2 before
    /// advancing YARD.
    pub fn at_base() -> Self {
        let temp = tempdir();
        let yard = temp.path().join("yard");
        let dev = temp.path().join("dev");

        let base_commit = create_yard_v1(&yard);

        git(
            temp.path(),
            &["clone", &yard.to_string_lossy(), &dev.to_string_lossy()],
        );
        configure_git_repo(&dev);

        let dev_commit = add_north_star(&dev);

        Self {
            temp,
            yard,
            dev,
            yard_commit: base_commit.clone(),
            base_commit,
            dev_commit,
        }
    }

    /// Standard fixture: `at_base()` plus a YARD v2 commit that bumps `engine`
    /// to `0.2.0`, adds `crates/helper`, and updates root `Cargo.toml` and
    /// `Cargo.lock`.
    pub fn standard() -> Self {
        let mut fixture = Self::at_base();
        fixture.advance_yard_standard();
        fixture
    }

    /// Template fixture at base: `yard` is at `base_commit`, and `dev` is a
    /// brand-new repository (sharing no git history with `yard`) whose single
    /// root commit holds the exact tree of `base_commit`, followed by
    /// `dev_commit` adding `app/north-star/`.
    pub fn from_template_at_base() -> Self {
        let temp = tempdir();
        let yard = temp.path().join("yard");
        let dev = temp.path().join("dev");

        let base_commit = create_yard_v1(&yard);

        git_init(&dev);
        let prefix = format!("--prefix={}/", dev.to_string_lossy().replace('\\', "/"));
        git(&yard, &["checkout-index", "--all", &prefix]);
        let root_commit = git_commit_all(&dev, "initial commit from template");
        assert_eq!(
            tree_oid(&dev, &root_commit),
            tree_oid(&yard, &base_commit),
            "template root commit must match YARD base tree"
        );
        let dev_commit = add_north_star(&dev);

        Self {
            temp,
            yard,
            dev,
            yard_commit: base_commit.clone(),
            base_commit,
            dev_commit,
        }
    }

    /// Template fixture with YARD advanced to v2.
    pub fn from_template() -> Self {
        let mut fixture = Self::from_template_at_base();
        fixture.advance_yard_standard();
        fixture
    }

    /// Template fixture whose `dev` root commit was modified before committing,
    /// so its root tree matches no YARD commit, and YARD has advanced to v2.
    pub fn from_template_with_mismatched_root() -> Self {
        let temp = tempdir();
        let yard = temp.path().join("yard");
        let dev = temp.path().join("dev");

        let base_commit = create_yard_v1(&yard);

        git_init(&dev);
        let prefix = format!("--prefix={}/", dev.to_string_lossy().replace('\\', "/"));
        git(&yard, &["checkout-index", "--all", &prefix]);
        write_file(&dev.join("docs/x.md"), "# Modified template root\n");
        let root_commit = git_commit_all(&dev, "initial commit with modified root");
        assert_ne!(
            tree_oid(&dev, &root_commit),
            tree_oid(&yard, &base_commit),
            "mismatched template root must differ from YARD base tree"
        );
        let dev_commit = add_north_star(&dev);

        let mut fixture = Self {
            temp,
            yard,
            dev,
            yard_commit: base_commit.clone(),
            base_commit,
            dev_commit,
        };
        fixture.advance_yard_standard();
        fixture
    }

    /// Advances YARD with the standard v2 commit (updated `engine`, new
    /// `crates/helper`, updated root `Cargo.toml`, regenerated `Cargo.lock`).
    pub fn advance_yard_standard(&mut self) -> String {
        write_file(&self.yard.join("Cargo.toml"), YARD_V2_ROOT_MANIFEST);
        write_file(
            &self.yard.join("crates/engine/Cargo.toml"),
            "[package]\nname = \"engine\"\nversion = \"0.2.0\"\nedition.workspace = true\n",
        );
        write_file(
            &self.yard.join("crates/engine/src/lib.rs"),
            "pub fn version() -> &'static str {\n    \"2\"\n}\n\npub fn feature() -> bool {\n    true\n}\n",
        );
        write_file(
            &self.yard.join("crates/helper/Cargo.toml"),
            "[package]\nname = \"helper\"\nversion = \"0.1.0\"\nedition.workspace = true\n",
        );
        write_file(
            &self.yard.join("crates/helper/src/lib.rs"),
            "pub fn help() -> &'static str {\n    \"ok\"\n}\n",
        );
        write_file(
            &self.yard.join("app/yard/Cargo.toml"),
            "[package]\nname = \"yard\"\nversion = \"0.1.0\"\nedition.workspace = true\n\n\
             [dependencies]\nengine.workspace = true\nhelper.workspace = true\n",
        );
        write_file(
            &self.yard.join("app/yard/src/main.rs"),
            "fn main() {\n    let _ = (engine::version(), helper::help());\n}\n",
        );
        cargo_generate_lockfile(&self.yard, true);
        self.yard_commit = git_commit_all(&self.yard, "yard v2: update engine and add helper");
        self.yard_commit.clone()
    }

    /// Creates a fixture whose YARD repository pins a local `file://` git
    /// dependency `shared_dep` at `1.0.0`, while `app/north-star` requires
    /// `shared_dep` `1.1`, causing `cargo metadata` to move YARD's pin offline
    /// without touching any remote network.
    pub fn moved_pin() -> Self {
        let temp = tempdir();
        let ext_dep = temp.path().join("ext-dep");
        let yard = temp.path().join("yard");
        let dev = temp.path().join("dev");

        git_init(&ext_dep);
        write_file(
            &ext_dep.join("Cargo.toml"),
            "[package]\nname = \"shared_dep\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
        );
        write_file(
            &ext_dep.join("src/lib.rs"),
            "pub fn version() -> u32 {\n    1\n}\n",
        );
        git_commit_all(&ext_dep, "shared_dep 1.0.0");

        let dep_url = file_url(&ext_dep);

        git_init(&yard);
        write_file(
            &yard.join("Cargo.toml"),
            &format!(
                "# YARD workspace manifest\n\
                 [workspace]\n\
                 resolver = \"3\"\n\
                 members = [\"app/*\", \"crates/*\"]\n\
                 default-members = [\"app/yard\"]\n\n\
                 [workspace.package]\n\
                 edition = \"2024\"\n\
                 publish = false\n\
                 license = \"GPL-3.0-or-later\"\n\n\
                 [workspace.dependencies]\n\
                 engine = {{ path = \"crates/engine\" }}\n\
                 shared_dep = {{ git = \"{dep_url}\", version = \"1.0\" }}\n"
            ),
        );
        write_file(
            &yard.join("app/yard/Cargo.toml"),
            "[package]\nname = \"yard\"\nversion = \"0.1.0\"\nedition.workspace = true\n\n\
             [dependencies]\nengine.workspace = true\nshared_dep.workspace = true\n",
        );
        write_file(
            &yard.join("app/yard/src/main.rs"),
            "fn main() {\n    let _ = (engine::version(), shared_dep::version());\n}\n",
        );
        write_file(
            &yard.join("crates/engine/Cargo.toml"),
            "[package]\nname = \"engine\"\nversion = \"0.1.0\"\nedition.workspace = true\n",
        );
        write_file(
            &yard.join("crates/engine/src/lib.rs"),
            "pub fn version() -> &'static str {\n    \"1\"\n}\n",
        );
        write_file(&yard.join("docs/x.md"), "# YARD Docs\n\nLine one.\n");
        cargo_generate_lockfile(&yard, false);
        let base_commit = git_commit_all(&yard, "initial yard pinning shared_dep 1.0.0");

        git(
            temp.path(),
            &["clone", &yard.to_string_lossy(), &dev.to_string_lossy()],
        );
        configure_git_repo(&dev);

        // Publish 1.1.0 in the local git dependency repository.
        write_file(
            &ext_dep.join("Cargo.toml"),
            "[package]\nname = \"shared_dep\"\nversion = \"1.1.0\"\nedition = \"2024\"\n",
        );
        write_file(
            &ext_dep.join("src/lib.rs"),
            "pub fn version() -> u32 {\n    2\n}\n",
        );
        git_commit_all(&ext_dep, "shared_dep 1.1.0");

        // Developer adds north-star requiring shared_dep 1.1.
        write_file(
            &dev.join("Cargo.toml"),
            &format!(
                "# YARD workspace manifest\n\
                 [workspace]\n\
                 resolver = \"3\"\n\
                 members = [\"app/*\", \"crates/*\", \"app/north-star/extensions/*\"]\n\
                 default-members = [\"app/north-star\"]\n\n\
                 [workspace.package]\n\
                 edition = \"2024\"\n\
                 publish = false\n\
                 license = \"GPL-3.0-or-later\"\n\n\
                 [workspace.dependencies]\n\
                 engine = {{ path = \"crates/engine\" }}\n\
                 shared_dep = {{ git = \"{dep_url}\", version = \"1.0\" }}\n"
            ),
        );
        write_file(
            &dev.join("app/north-star/Cargo.toml"),
            &format!(
                "[package]\nname = \"north-star\"\nversion = \"0.1.0\"\nedition.workspace = true\n\n\
                 [dependencies]\nengine.workspace = true\n\
                 notes = {{ path = \"extensions/notes\" }}\n\
                 shared_dep = {{ git = \"{dep_url}\", version = \"1.1\" }}\n"
            ),
        );
        write_file(
            &dev.join("app/north-star/src/main.rs"),
            "fn main() {\n    let _ = (engine::version(), notes::name(), shared_dep::version());\n}\n",
        );
        write_file(
            &dev.join("app/north-star/extensions/notes/Cargo.toml"),
            "[package]\nname = \"notes\"\nversion = \"0.1.0\"\nedition.workspace = true\n",
        );
        write_file(
            &dev.join("app/north-star/extensions/notes/src/lib.rs"),
            "pub fn name() -> &'static str {\n    \"notes\"\n}\n",
        );
        cargo_generate_lockfile(&dev, false);
        let dev_commit = git_commit_all(&dev, "add north-star requiring shared_dep 1.1");

        // YARD advances while keeping Cargo.lock pinned to shared_dep 1.0.0.
        write_file(
            &yard.join("crates/engine/src/lib.rs"),
            "pub fn version() -> &'static str {\n    \"2\"\n}\n",
        );
        let yard_commit = git_commit_all(&yard, "yard v2 keeping shared_dep 1.0.0");

        Self {
            temp,
            yard,
            dev,
            base_commit,
            dev_commit,
            yard_commit,
        }
    }

    pub fn temp_path(&self) -> &Path {
        self.temp.path()
    }

    pub fn yard_remote(&self) -> String {
        self.yard.to_string_lossy().into_owned()
    }

    pub fn expected_branch(&self) -> String {
        format!("yard-update/{}", &self.yard_commit[..12])
    }
}
