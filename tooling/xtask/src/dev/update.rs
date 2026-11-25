//! Brings a later YARD into a separate branch for review.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

use super::bundle::{
    Captured, Failure, Interrupt, Runner, SystemRunner, log_outcome, not_interrupted,
};
use super::new_app::{Manifest, edit_manifest, expanded};

pub const YARD_REMOTE: &str = "https://github.com/Riso-platform-factory/YARD.git";
const REMOTE: &str = "yard-upstream";
const BRANCH_PREFIX: &str = "yard-update/";
const PREPARED: &str = "YARD-Update-Prepared: true";
const MERGE_SUBJECT: &str = "Update YARD to ";
const DEVELOPER_APP: &[&str] = &["app", ":(exclude)app/yard"];

#[derive(clap::Args, Debug, Clone)]
pub struct UpdateArgs {
    #[arg(long = "ref", default_value = "main")]
    pub reference: String,
    #[arg(long)]
    pub remote: Option<String>,
    #[arg(long)]
    pub base: Option<String>,
    #[arg(long = "continue")]
    pub resume: bool,
}

impl Default for UpdateArgs {
    fn default() -> Self {
        Self {
            reference: "main".into(),
            remote: None,
            base: None,
            resume: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Updated {
    UpToDate {
        yard_commit: String,
    },
    Done {
        branch: String,
        yard_commit: String,
        start: String,
        carried: Vec<String>,
        lock_changes: Vec<String>,
    },
    Conflicts {
        branch: String,
        paths: Vec<String>,
        manifest_keys: Vec<String>,
    },
}

pub fn update(root: &Path, args: &UpdateArgs, runner: &mut dyn Runner) -> Result<Updated, Failure> {
    let interrupt = Interrupt::default();
    let mut repo = Repo {
        root,
        runner,
        interrupt: &interrupt,
    };
    let result = repo.update(args);
    result.map(|(updated, _)| updated)
}

pub(super) fn run_update(root: &Path, args: &UpdateArgs, interrupt: Interrupt) -> u8 {
    let mut runner = SystemRunner {
        interrupt: interrupt.clone(),
    };
    let mut repo = Repo {
        root,
        runner: &mut runner,
        interrupt: &interrupt,
    };
    match repo.update(args) {
        Ok((Updated::UpToDate { yard_commit }, _)) => {
            println!("Already up to date with YARD {yard_commit}.");
            0
        }
        Ok((
            Updated::Conflicts {
                paths,
                manifest_keys,
                branch,
            },
            report,
        )) => {
            eprintln!("Conflicts on {branch}:");
            for path in paths {
                eprintln!("  {path}");
            }
            for key in manifest_keys {
                eprintln!("  Cargo.toml: {key}");
            }
            if let Some((yard, initial)) = report.manifest_conflict {
                if initial {
                    eprintln!("The working-tree Cargo.toml holds your starting version.");
                }
                eprintln!("See YARD's version with git show {yard}:Cargo.toml.");
            }
            eprintln!(
                "Resolve and stage the conflicts, then run cargo xtask update --continue. Do not run git commit."
            );
            1
        }
        Ok((
            Updated::Done {
                branch,
                yard_commit,
                start,
                carried,
                lock_changes,
            },
            report,
        )) => {
            println!("Updated {branch} to YARD {yard_commit}.");
            println!("YARD commits taken:\n{}", none_if_empty(&report.commits));
            println!("Changes by area:");
            for (area, paths) in report.areas {
                if !paths.is_empty() {
                    println!("  {area}: {}", paths.join(", "));
                }
            }
            println!(
                "Developer changes carried in YARD files: {}",
                none_if_empty(&carried.join(", "))
            );
            println!("Lock changes:\n{}", none_if_empty(&lock_changes.join("\n")));
            println!(
                "Passed: cargo test -p yard --locked; cargo check --workspace --all-targets --locked"
            );
            let range = shell_word(&format!("{start}..{branch}"));
            let start = shell_word(&start);
            println!("Review: git diff {range}");
            println!("Integrate with a merge to retain YARD history:");
            println!("  git checkout {start} && git merge {branch}");
            0
        }
        Err(failure) => {
            eprintln!("update: {failure}");
            if failure.step == "verify" {
                eprintln!("The update branch remains checked out for inspection.");
            }
            runner.interrupt.exit_code()
        }
    }
}

fn none_if_empty(text: &str) -> &str {
    if text.is_empty() { "none" } else { text }
}

#[derive(Default)]
struct Report {
    commits: String,
    areas: Vec<(&'static str, Vec<String>)>,
    manifest_conflict: Option<(String, bool)>,
}

struct Repo<'a> {
    root: &'a Path,
    runner: &'a mut dyn Runner,
    interrupt: &'a Interrupt,
}

struct Merge {
    branch: String,
    start: String,
    yard: String,
    base: String,
}

struct ResumeLock {
    index: String,
    worktree: Option<(Vec<u8>, fs::Permissions)>,
}

fn refusal(reason: impl std::fmt::Display) -> Failure {
    Failure::new("arguments", reason)
}

fn shell_word(word: &str) -> String {
    if word
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"/-._".contains(&byte))
    {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

enum RetryMerge {
    None,
    Reset,
    Abort,
    DiscardCommit,
}

fn retry_commands(start: &str, branch: &str, merge: RetryMerge, lock: bool) -> String {
    let start = shell_word(start);
    let branch = shell_word(branch);
    let mut message = String::from("\nBefore retrying:");
    match merge {
        RetryMerge::None => {}
        RetryMerge::Reset => message.push_str(
            "\nDiscard the unfinished merge and any tracked edits:\n  git reset --hard HEAD",
        ),
        RetryMerge::DiscardCommit => message.push_str(
            "\nDiscard the manually created merge commit. You will need to redo the conflict resolutions.\n  git reset --hard HEAD~1",
        ),
        RetryMerge::Abort => message.push_str(
            "\nThe update can be finished only from the start branch. You will need to redo the conflict resolutions.\n  git restore --worktree -- Cargo.lock\n  git merge --abort",
        ),
    }
    message.push_str(&format!(
        "\n  git checkout {start}\n  git branch -D {branch}"
    ));
    if lock {
        message.push_str(&format!("\nChange the named requirement on {start} and commit it before running the update again."));
    }
    message
}

fn safe_remote(remote: &str) -> String {
    if let Some((helper, target)) = remote.split_once("::")
        && !helper.is_empty()
        && helper
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"+-._".contains(&byte))
    {
        return format!("{helper}::{}", safe_remote(target));
    }
    if let Some((scheme, rest)) = remote.split_once("://") {
        let host_and_path = rest.rsplit('@').next().unwrap_or_default();
        let safe = host_and_path.split(['?', '#']).next().unwrap_or_default();
        return format!("{scheme}://{safe}");
    }
    if let Some((user, host)) = remote.split_once('@')
        && !user.contains(['/', '\\'])
        && host.contains(':')
    {
        return host.to_owned();
    }
    remote.to_owned()
}

fn scrub_urls(text: &str) -> String {
    let mut clean = String::new();
    let separator = |ch: char| ch.is_whitespace() || matches!(ch, '"' | '`' | '<' | '>' | '\\');
    for part in text.split_inclusive(separator) {
        let body = part.trim_end_matches(separator);
        let mut start = 0;
        for (marker, _) in body.match_indices("://").skip(1) {
            let scheme = body[..marker]
                .trim_end_matches(|ch: char| {
                    ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.')
                })
                .len();
            if scheme > start {
                clean.push_str(&safe_remote(&body[start..scheme]));
                start = scheme;
            }
        }
        let rest = &body[start..];
        if rest.contains("://") {
            let url = rest.trim_end_matches(['\'', ')', ',', ';']);
            clean.push_str(&safe_remote(url));
            clean.push_str(&rest[url.len()..]);
        } else {
            clean.push_str(rest);
        }
        clean.push_str(&part[body.len()..]);
    }
    clean
}

impl Repo<'_> {
    fn command(&self, program: &str, args: &[&str]) -> Command {
        let mut command = Command::new(program);
        command.current_dir(self.root).args(args);
        command
    }

    fn capture(
        &mut self,
        program: &str,
        args: &[&str],
        step: &'static str,
    ) -> Result<Captured, Failure> {
        not_interrupted(step, self.interrupt)?;
        let mut command = self.command(program, args);
        let result = self.runner.capture(&mut command);
        not_interrupted(step, self.interrupt)?;
        result.map_err(|err| Failure::new(step, scrub_urls(&err)))
    }

    fn git(&mut self, args: &[&str]) -> Result<String, Failure> {
        not_interrupted("git", self.interrupt)?;
        self.git_even_if_interrupted(args)
    }

    fn git_even_if_interrupted(&mut self, args: &[&str]) -> Result<String, Failure> {
        let mut command = self.command("git", args);
        let result = self
            .runner
            .capture(&mut command)
            .map_err(|err| Failure::new("git", scrub_urls(&err)))?;
        if !result.success {
            return Err(Failure::new(
                "git",
                format!(
                    "git {}: {}",
                    args[0],
                    scrub_urls(String::from_utf8_lossy(&result.stderr).trim())
                ),
            ));
        }
        String::from_utf8(result.stdout).map_err(|error| Failure::new("git", error))
    }

    fn git_trim(&mut self, args: &[&str]) -> Result<String, Failure> {
        Ok(self.git(args)?.trim_end_matches(['\r', '\n']).into())
    }

    fn git_path(&mut self, name: &str) -> Result<PathBuf, Failure> {
        Ok(self
            .root
            .join(self.git_trim(&["rev-parse", "--git-path", name])?))
    }

    fn predicate(&mut self, args: &[&str]) -> Result<bool, Failure> {
        let result = self.capture("git", args, "git")?;
        if result.success || result.status == "exit status: 1" || result.status == "exit code: 1" {
            Ok(result.success)
        } else {
            Err(Failure::new(
                "git",
                format!(
                    "git {}: {}",
                    args[0],
                    scrub_urls(String::from_utf8_lossy(&result.stderr).trim())
                ),
            ))
        }
    }

    fn ancestor(&mut self, older: &str, newer: &str) -> Result<bool, Failure> {
        self.predicate(&["merge-base", "--is-ancestor", older, newer])
    }

    fn paths(&mut self, args: &[&str]) -> Result<Vec<String>, Failure> {
        Ok(self
            .git(args)?
            .split_terminator('\0')
            .map(str::to_owned)
            .collect())
    }

    fn update(&mut self, args: &UpdateArgs) -> Result<(Updated, Report), Failure> {
        let configured = if args.resume && args.remote.is_none() {
            self.git_trim(&["config", "--get", "remote.yard-upstream.url"])
                .ok()
        } else {
            None
        };
        let remote = args
            .remote
            .as_deref()
            .or(configured.as_deref())
            .unwrap_or(YARD_REMOTE);
        let reference = args.reference.as_str();
        let span = tracing::info_span!("yard-update", remote = %safe_remote(remote), r#ref = %scrub_urls(reference));
        let _group = span.enter();
        let result = self.perform(args, remote, reference);
        log_outcome(&result, self.interrupt);
        result
    }

    fn perform(
        &mut self,
        args: &UpdateArgs,
        remote: &str,
        reference: &str,
    ) -> Result<(Updated, Report), Failure> {
        // TODO(upstream-updates-template): Support --base for template repositories. Implement Part 2.
        if args.base.is_some() {
            return Err(refusal(
                "--base is for repositories made from the GitHub template, which this version does not support yet",
            ));
        }
        let current = self.capture(
            "git",
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
            "git",
        )?;
        if !current.success {
            return Err(refusal("HEAD is detached; check out a branch first"));
        }
        let current = String::from_utf8_lossy(&current.stdout)
            .trim_end()
            .to_owned();
        if self.git_trim(&["rev-parse", "--is-shallow-repository"])? == "true" {
            return Err(refusal("shallow repository; run git fetch --unshallow"));
        }
        let merge_head = self.git_path("MERGE_HEAD")?;
        if args.resume {
            if current.starts_with(BRANCH_PREFIX) && !merge_head.is_file() {
                let head = self.git(&["log", "-1", "--format=%P%n%B"])?;
                let mut lines = head.lines();
                let parents: Vec<_> = lines
                    .next()
                    .unwrap_or_default()
                    .split_whitespace()
                    .collect();
                if let [_, yard] = parents.as_slice()
                    && current == format!("{BRANCH_PREFIX}{}", &yard[..12])
                    && lines
                        .next()
                        .and_then(|line| line.strip_prefix(MERGE_SUBJECT))
                        == Some(*yard)
                    && lines.clone().any(|line| line == PREPARED)
                    && let Some(start) =
                        lines.find_map(|line| line.strip_prefix("YARD-Update-Start: "))
                {
                    return Err(refusal(format!(
                        "git commit ended the update merge before the lock and verification steps{}",
                        retry_commands(start, &current, RetryMerge::DiscardCommit, false)
                    )));
                }
            }
            if !current.starts_with(BRANCH_PREFIX) || !merge_head.is_file() {
                return Err(refusal("no merge in progress on a yard-update branch"));
            }
            let yard = self.git_trim(&["rev-parse", "--verify", "MERGE_HEAD^{commit}"])?;
            if current != format!("{BRANCH_PREFIX}{}", &yard[..12]) {
                return Err(refusal(
                    "the merge in progress does not match the update branch",
                ));
            }
            let message_path = self.git_path("MERGE_MSG")?;
            let message = if message_path.exists() {
                self.read(&message_path, "git")?
            } else {
                String::new()
            };
            let start = message
                .lines()
                .find_map(|line| line.strip_prefix("YARD-Update-Start: "));
            if !message.lines().any(|line| line == PREPARED) {
                let previous = self
                    .git_trim(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{-1}"])
                    .ok()
                    .filter(|branch| !branch.is_empty() && branch != &current);
                let start = start.or(previous.as_deref()).unwrap_or("YOUR_START_BRANCH");
                return Err(refusal(format!(
                    "the previous update was interrupted before conflict setup finished{}",
                    retry_commands(start, &current, RetryMerge::Reset, false)
                )));
            }
            let start =
                start.ok_or_else(|| refusal("the merge in progress has no YARD update record"))?;
            let mut lock = None;
            let result = (|| {
                self.check_yard_app(&yard)?;
                let base = self.base(&yard)?;
                tracing::debug!("checked");
                tracing::debug!("base: shared history");
                lock = Some(self.resume_lock()?);
                self.finish(
                    Merge {
                        branch: current.clone(),
                        start: start.into(),
                        yard: yard.clone(),
                        base,
                    },
                    true,
                )
            })();
            return result.map_err(|mut failure| {
                if merge_head.is_file() {
                    if let Some(lock) = lock {
                        match self.restore_resume_lock(lock, yard.len()) {
                            Ok(()) => tracing::debug!("restored pre-continue lock"),
                            Err(error) => {
                                log::error!("could not restore pre-continue lock: {error}");
                                failure
                                    .reason
                                    .push_str(&format!("\nCould not restore Cargo.lock: {error}"));
                            }
                        }
                    }
                    if failure.step == "lock" && !self.interrupt.is_set() {
                        failure.reason.push_str(&retry_commands(start, &current, RetryMerge::Abort, true));
                    } else {
                        failure.reason.push_str("\nRun cargo xtask update --continue again to resume with your staged resolutions.");
                    }
                }
                failure
            });
        }
        for marker in [
            "MERGE_HEAD",
            "rebase-merge",
            "rebase-apply",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "sequencer",
        ] {
            if self.git_path(marker)?.exists() {
                return Err(refusal(format!(
                    "a Git operation is in progress ({marker})"
                )));
            }
        }
        if current.starts_with(BRANCH_PREFIX) {
            return Err(refusal(format!("already on update branch {current}")));
        }
        if !self
            .git(&["status", "--porcelain", "--untracked-files=all"])?
            .is_empty()
        {
            return Err(refusal(
                "working tree has changes; commit or stash them before updating",
            ));
        }

        if reference.contains(':')
            || reference.starts_with(['-', '+'])
            || reference
                .chars()
                .any(|ch| ch.is_whitespace() || ch.is_control())
            || !self
                .capture(
                    "git",
                    &["check-ref-format", "--allow-onelevel", reference],
                    "arguments",
                )?
                .success
        {
            return Err(refusal("invalid --ref; expected a branch or tag name"));
        }

        let remotes = self.git(&["remote"])?;
        let has_remote = remotes.lines().any(|name| name == REMOTE);
        if has_remote && self.git_trim(&["config", "--get", "remote.yard-upstream.url"])? != remote
        {
            return Err(refusal(
                "yard-upstream has another URL; use --remote with its configured URL",
            ));
        }
        tracing::debug!("checked");

        let source = if has_remote { REMOTE } else { remote };
        eprintln!("Fetching {} from {}", reference, safe_remote(remote));
        let fetch = self.capture(
            "git",
            &[
                "fetch",
                "--no-tags",
                "--refmap=",
                "--no-recurse-submodules",
                "--",
                source,
                reference,
            ],
            "fetch",
        )?;
        if !fetch.success {
            // Git can include its credential-bearing effective URL in diagnostics.
            return Err(Failure::new(
                "fetch",
                format!(
                    "git fetch from {} failed ({})",
                    safe_remote(remote),
                    fetch.status
                ),
            ));
        }
        tracing::debug!("fetched");
        let yard = self.git_trim(&["rev-parse", "--verify", "FETCH_HEAD^{commit}"])?;
        let base = self.base(&yard)?;
        tracing::debug!("base: shared history");
        self.check_yard_app(&yard)?;
        if self.ancestor(&yard, "HEAD")? {
            return Ok((Updated::UpToDate { yard_commit: yard }, Report::default()));
        }
        self.check_rewrite(&yard)?;
        let branch = format!("{BRANCH_PREFIX}{}", &yard[..12]);
        if self.predicate(&[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])? {
            return Err(refusal(format!("update branch {branch} already exists")));
        }
        let mut deleted_apps = vec![
            "diff",
            "--name-only",
            "--no-renames",
            "--diff-filter=D",
            "-z",
            &base,
            &yard,
            "--",
        ];
        deleted_apps.extend(DEVELOPER_APP);
        for path in self.paths(&deleted_apps)? {
            let literal = format!(":(literal){path}");
            if !self
                .git(&["ls-files", "--cached", "--", &literal])?
                .is_empty()
            {
                return Err(refusal(format!(
                    "YARD would remove developer application path {path}"
                )));
            }
        }
        for shared in ["Cargo.toml", "Cargo.lock"] {
            for revision in ["HEAD", &yard] {
                let entry = self.git(&["ls-tree", revision, "--", shared])?;
                if !entry.starts_with("100644 ") && !entry.starts_with("100755 ") {
                    return Err(refusal(format!(
                        "{revision} must contain {shared} as a regular file"
                    )));
                }
            }
        }
        self.git(&["checkout", "--no-overwrite-ignore", "-b", &branch])?;
        tracing::debug!("branched");
        let message = format!("{MERGE_SUBJECT}{yard}\n\nYARD-Update-Start: {current}");
        let result = (|| {
            let merge = self.capture(
                "git",
                &[
                    "merge",
                    "--no-overwrite-ignore",
                    "--no-ff",
                    "--no-commit",
                    "-m",
                    &message,
                    &yard,
                ],
                "merge",
            )?;
            if !merge.success && !merge_head.is_file() {
                return Err(Failure::new(
                    "merge",
                    scrub_urls(String::from_utf8_lossy(&merge.stderr).trim()),
                ));
            }
            tracing::debug!("merged");
            self.finish(
                Merge {
                    branch: branch.clone(),
                    start: current.clone(),
                    yard,
                    base,
                },
                false,
            )
        })();
        let mut updated = result.map_err(|mut failure| {
            if failure.step != "verify" {
                let mut needs_reset = false;
                if merge_head.is_file() {
                    // The update started from a clean checkout, so the
                    // in-progress changes belong to this run.
                    match self.git_even_if_interrupted(&["reset", "--hard", "HEAD"]) {
                        Ok(_) => tracing::debug!("rolled back incomplete merge"),
                        Err(error) => {
                            log::error!("could not roll back incomplete merge: {error}");
                            failure
                                .reason
                                .push_str(&format!("\nCould not roll back the merge: {error}"));
                            needs_reset = true;
                        }
                    }
                }
                failure.reason.push_str(&retry_commands(
                    &current,
                    &branch,
                    if needs_reset {
                        RetryMerge::Reset
                    } else {
                        RetryMerge::None
                    },
                    failure.step == "lock" && !self.interrupt.is_set(),
                ));
            }
            failure
        });
        if !has_remote
            && (updated.is_ok() || matches!(&updated, Err(failure) if failure.step == "verify"))
            && let Err(error) =
                self.git_even_if_interrupted(&["remote", "add", "--", REMOTE, remote])
        {
            match &mut updated {
                Ok(_) => return Err(error),
                Err(failure) => failure
                    .reason
                    .push_str(&format!("\nCould not configure {REMOTE}: {error}")),
            }
        }
        updated
    }

    fn base(&mut self, yard: &str) -> Result<String, Failure> {
        if !self.predicate(&["merge-base", "HEAD", yard])? {
            return Err(refusal("this repository shares no history with YARD"));
        }
        self.git_trim(&["merge-base", "HEAD", yard])
    }

    fn check_rewrite(&mut self, yard: &str) -> Result<(), Failure> {
        let merges = self.git(&["log", "--topo-order", "--merges", "--format=%P%x09%s"])?;
        for line in merges.lines() {
            if let Some((parents, subject)) = line.split_once('\t')
                && let Some(previous) = subject.strip_prefix(MERGE_SUBJECT)
                && parents.split_whitespace().nth(1) == Some(previous)
                && !self.ancestor(previous, yard)?
            {
                return Err(refusal(format!(
                    "YARD history was rewritten: {yard} no longer contains {previous}"
                )));
            }
        }
        Ok(())
    }

    fn check_yard_app(&mut self, yard: &str) -> Result<(), Failure> {
        let paths = self.paths(&["ls-tree", "-r", "-z", "--name-only", yard, "--", "app"])?;
        let foreign: Vec<_> = paths
            .iter()
            .filter(|path| !path.starts_with("app/yard/"))
            .collect();
        if !foreign.is_empty() {
            return Err(refusal(format!(
                "YARD commit contains developer application paths: {foreign:?}"
            )));
        }
        Ok(())
    }

    fn read(&self, path: &Path, step: &'static str) -> Result<String, Failure> {
        fs::read_to_string(path)
            .map_err(|error| Failure::new(step, format!("{}: {error}", path.display())))
    }

    fn write(&self, name: &str, text: &str, step: &'static str) -> Result<(), Failure> {
        let path = self.root.join(name);
        fs::write(&path, text)
            .map_err(|error| Failure::new(step, format!("{}: {error}", path.display())))
    }

    fn blob(&mut self, revision: &str, path: &str) -> Result<String, Failure> {
        self.git(&["show", &format!("{revision}:{path}")])
    }
}

impl Repo<'_> {
    fn finish(&mut self, merge: Merge, resume: bool) -> Result<(Updated, Report), Failure> {
        let Merge {
            branch,
            start,
            yard,
            base,
        } = merge;
        let mut paths = self.paths(&["diff", "--name-only", "--diff-filter=U", "-z"])?;
        paths.retain(|path| path != "Cargo.lock" && (resume || path != "Cargo.toml"));
        let upstream_paths = self.paths(&[
            "diff",
            "--name-only",
            "--no-renames",
            "-z",
            &base,
            &yard,
            "--",
        ])?;
        let developer_paths = self.paths(&[
            "diff",
            "--name-only",
            "--no-renames",
            "-z",
            &base,
            "HEAD",
            "--",
        ])?;
        if !resume {
            let mut manual = Vec::new();
            let removed = self.paths(&[
                "diff",
                "--no-renames",
                "--diff-filter=D",
                "--name-only",
                "-z",
                &base,
                &yard,
                "--",
            ])?;
            manual.extend(
                removed
                    .into_iter()
                    .filter(|path| yard_owned(path) && developer_paths.contains(path)),
            );
            let added = |tip| {
                [
                    "diff",
                    "--no-renames",
                    "--diff-filter=A",
                    "--name-only",
                    "-z",
                    base.as_str(),
                    tip,
                    "--",
                ]
            };
            let yard_added: BTreeSet<_> = self.paths(&added(&yard))?.into_iter().collect();
            for path in self.paths(&added("HEAD"))? {
                if !path.contains('/')
                    && !yard_owned(&path)
                    && !matches!(path.as_str(), "Cargo.toml" | "Cargo.lock")
                    && yard_added.contains(&path)
                {
                    manual.push(path);
                }
            }
            for path in manual {
                if !paths.contains(&path) {
                    self.mark_unmerged(&path, [&base, "HEAD", &yard])?;
                    paths.push(path);
                }
            }
        }

        let mut app_diff = vec!["diff", "--name-only", "--no-renames", "-z", "HEAD", "--"];
        #[cfg(unix)]
        app_diff.splice(..0, ["-c", "core.fileMode=true"]);
        app_diff.extend(DEVELOPER_APP);
        paths.extend(self.paths(&app_diff)?);
        let mut index_diff = vec![
            "diff",
            "--cached",
            "--name-only",
            "--no-renames",
            "-z",
            "HEAD",
            "--",
        ];
        index_diff.extend(DEVELOPER_APP);
        paths.extend(self.paths(&index_diff)?);
        let mut untracked = vec!["ls-files", "--others", "--exclude-standard", "-z", "--"];
        untracked.extend(DEVELOPER_APP);
        paths.extend(self.paths(&untracked)?);

        let mut manifest_keys = Vec::new();
        if resume {
            paths.extend(self.paths(&[
                "diff",
                "--name-only",
                "--diff-filter=ACDMRT",
                "--no-renames",
                "-z",
                "--",
                ".",
                ":(exclude)Cargo.lock",
            ])?);
            if !paths.iter().any(|path| path == "Cargo.toml")
                && !self.resolved_manifest(&base, &yard, &mut manifest_keys)?
            {
                paths.push("Cargo.toml".into());
            }
        } else {
            self.restore(&yard, "Cargo.lock")?;
            let yard_text = self.blob(&yard, "Cargo.toml")?;
            let yard_manifest = parse_manifest::<toml::Table>(&yard_text)?;
            let original = parse_manifest::<toml::Table>(&self.blob("HEAD", "Cargo.toml")?)?;
            let baseline = parse_manifest(&self.blob(&base, "Cargo.toml")?)?;
            manifest_differences(
                "",
                Some(&toml::Value::Table(platform_manifest(baseline)?)),
                Some(&toml::Value::Table(platform_manifest(original.clone())?)),
                &mut manifest_keys,
            );
            let merged = merged_manifest(
                self.root,
                yard_text,
                &original,
                &yard_manifest,
                &mut manifest_keys,
            );
            if manifest_keys.is_empty() {
                let text = merged?;
                self.restore(&yard, "Cargo.toml")?;
                self.write("Cargo.toml", &text, "manifest")?;
                self.git(&["add", "--", "Cargo.toml"])?;
            } else {
                self.git(&["restore", "--source=HEAD", "--worktree", "--", "Cargo.toml"])?;
                self.mark_unmerged("Cargo.toml", [&base, "HEAD", &yard])?;
            }
        }
        paths.sort();
        paths.dedup();
        if !paths.is_empty() || !manifest_keys.is_empty() {
            if !resume {
                let path = self.git_path("MERGE_MSG")?;
                fs::OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .and_then(|mut message| writeln!(message, "\n{PREPARED}"))
                    .map_err(|error| {
                        Failure::new("merge", format!("{}: {error}", path.display()))
                    })?;
            }
            tracing::warn!(?paths, ?manifest_keys, "update conflicts");
            let manifest_conflict = (!manifest_keys.is_empty()
                || paths.iter().any(|path| path == "Cargo.toml"))
            .then_some((yard, !resume));
            return Ok((
                Updated::Conflicts {
                    branch,
                    paths,
                    manifest_keys,
                },
                Report {
                    manifest_conflict,
                    ..Report::default()
                },
            ));
        }
        tracing::debug!("manifest");

        let old_lock = lockfile(&self.blob("HEAD", "Cargo.lock")?)?;
        let yard_lock = lockfile(&self.blob(&yard, "Cargo.lock")?)?;
        self.restore(&yard, "Cargo.lock")?;
        let lock_changes = self.resolve_lock(&old_lock, &yard_lock)?;
        self.git(&["add", "--", "Cargo.lock"])?;
        tracing::debug!("lock");

        let carried = self
            .paths(&[
                "diff",
                "--cached",
                "--name-only",
                "--no-renames",
                "-z",
                &yard,
                "--",
            ])?
            .into_iter()
            .filter(|path| yard_owned(path) && developer_paths.contains(path))
            .collect();
        let commits =
            scrub_urls(&self.git_trim(&["log", "--oneline", &format!("{base}..{yard}")])?);
        let mut areas = Vec::<(&str, Vec<String>)>::new();
        for path in upstream_paths {
            let area = match path.split('/').next().unwrap_or_default() {
                "crates" => "engines",
                "extensions" => "extensions",
                "tooling" | ".cargo" | ".github" => "tools",
                "docs" => "docs",
                "app" | "examples" => "sample",
                "Cargo.toml" => "root manifest",
                "Cargo.lock" => "lock file",
                _ => "other",
            };
            if let Some((_, paths)) = areas.iter_mut().find(|(name, _)| *name == area) {
                paths.push(path);
            } else {
                areas.push((area, vec![path]));
            }
        }

        let message = format!("{MERGE_SUBJECT}{yard}\n\nYARD-Update-Start: {start}");
        self.git(&["commit", "-m", &message])?;
        for args in [
            &["test", "-p", "yard", "--locked"][..],
            &["check", "--workspace", "--all-targets", "--locked"],
        ] {
            eprintln!("Running cargo {}", args.join(" "));
            not_interrupted("verify", self.interrupt)?;
            let mut command = self.command("cargo", args);
            let result = self.runner.run(&mut command);
            not_interrupted("verify", self.interrupt)?;
            result.map_err(|error| Failure::new("verify", scrub_urls(&error)))?;
        }
        tracing::debug!("verified");
        Ok((
            Updated::Done {
                branch,
                yard_commit: yard,
                start,
                carried,
                lock_changes,
            },
            Report {
                commits,
                areas,
                ..Report::default()
            },
        ))
    }

    fn resume_lock(&mut self) -> Result<ResumeLock, Failure> {
        let index = self.git(&["ls-files", "--stage", "-z", "--", "Cargo.lock"])?;
        let path = self.root.join("Cargo.lock");
        let error = |error: std::io::Error| Failure::new("lockfile", error);
        let worktree = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() => {
                Some((fs::read(&path).map_err(error)?, metadata.permissions()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Ok(_) => return Err(refusal("Cargo.lock must be a regular file")),
            Err(err) => return Err(error(err)),
        };
        Ok(ResumeLock { index, worktree })
    }

    fn restore_resume_lock(&mut self, lock: ResumeLock, oid_len: usize) -> Result<(), Failure> {
        let refresh = lock.index.contains(" 0\t") && lock.worktree.is_some();
        let records = format!("0 {}\tCargo.lock\0{}", "0".repeat(oid_len), lock.index);
        let mut command = self.command("git", &["update-index", "-z", "--index-info"]);
        self.runner
            .input(&mut command, records.as_bytes())
            .map_err(|error| Failure::new("lockfile", error))?;
        let path = self.root.join("Cargo.lock");
        match lock.worktree {
            Some((bytes, permissions)) => {
                fs::write(&path, bytes).and_then(|()| fs::set_permissions(&path, permissions))
            }
            None => match fs::remove_file(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                result => result,
            },
        }
        .map_err(|error| Failure::new("lockfile", error))?;
        // Index records restored by --index-info lack cached file stats.
        if refresh {
            self.git_even_if_interrupted(&["add", "--refresh", "--", "Cargo.lock"])?;
        }
        Ok(())
    }

    fn resolved_manifest(
        &mut self,
        base: &str,
        yard: &str,
        keys: &mut Vec<String>,
    ) -> Result<bool, Failure> {
        let entry = self.git(&["ls-files", "--stage", "--", "Cargo.toml"])?;
        if !(entry.starts_with("100644 ") || entry.starts_with("100755 ")) {
            return Ok(false);
        }
        let path = self.root.join("Cargo.toml");
        if !fs::symlink_metadata(&path).is_ok_and(|entry| entry.is_file()) {
            return Ok(false);
        }
        let staged = self.capture("git", &["show", ":Cargo.toml"], "git")?;
        let Ok(staged_text) = std::str::from_utf8(&staged.stdout) else {
            return Ok(false);
        };
        let Ok(staged_manifest) = parse_manifest::<toml::Table>(staged_text) else {
            return Ok(false);
        };
        if !staged.success {
            return Ok(false);
        }
        let original = parse_manifest::<toml::Table>(&self.blob("HEAD", "Cargo.toml")?)?;
        let upstream = parse_manifest::<toml::Table>(&self.blob(yard, "Cargo.toml")?)?;
        let baseline = parse_manifest::<toml::Table>(&self.blob(base, "Cargo.toml")?)?;
        let original_workspace = workspace(&original)?;
        let Ok(staged_workspace) = workspace(&staged_manifest) else {
            return Ok(false);
        };
        let defaults = original_workspace.get("default-members");
        if defaults != staged_workspace.get("default-members") {
            keys.push(manifest_value("workspace.default-members", defaults));
        }
        let staged_members = entries(staged_workspace, "members").unwrap_or_default();
        let original_members = entries(original_workspace, "members")?;
        for member in &original_members {
            if app_member(member) && !staged_members.contains(member) {
                keys.push(scrub_urls(&format!(
                    "workspace.members must include {member:?}"
                )));
            }
        }
        let upstream_workspace = workspace(&upstream)?;
        let original_exclude = original_workspace.get("exclude");
        let kept_exclude_resolution = original_exclude == staged_workspace.get("exclude")
            && original_exclude != upstream_workspace.get("exclude")
            && newly_excludes_app_member(
                &developer_app_members(self.root, &original_members)?,
                &expanded(self.root, entries(original_workspace, "exclude")?)?,
                &expanded(self.root, entries(upstream_workspace, "exclude")?)?,
            );
        required_yard_keys(
            "",
            Some(&toml::Value::Table(platform_manifest(baseline)?)),
            Some(&toml::Value::Table(platform_manifest(original)?)),
            Some(&toml::Value::Table(platform_manifest(upstream)?)),
            Some(&toml::Value::Table(platform_manifest(
                staged_manifest.clone(),
            )?)),
            kept_exclude_resolution,
            keys,
        );
        Ok(keys.is_empty()
            && validate_workspace(self.root, &staged_manifest).is_ok()
            && self.read(&path, "manifest").is_ok_and(|text| {
                parse_manifest::<toml::Table>(&text)
                    .and_then(|manifest| validate_workspace(self.root, &manifest))
                    .is_ok()
            }))
    }

    fn mark_unmerged(&mut self, path: &str, revisions: [&str; 3]) -> Result<(), Failure> {
        let mut records = String::new();
        for (index, revision) in revisions.into_iter().enumerate() {
            let entry =
                self.git(&["--literal-pathspecs", "ls-tree", "-z", revision, "--", path])?;
            if let Some((fields, _)) = entry.split_once('\t') {
                let mut fields = fields.split_whitespace();
                let mode = fields.next().unwrap_or_default();
                let oid = fields.nth(1).unwrap_or_default();
                if records.is_empty() {
                    records.push_str(&format!("0 {}\t{path}\0", "0".repeat(oid.len())));
                }
                records.push_str(&format!("{mode} {oid} {}\t{path}\0", index + 1));
            }
        }
        not_interrupted("merge", self.interrupt)?;
        let mut command = self.command("git", &["update-index", "-z", "--index-info"]);
        self.runner
            .input(&mut command, records.as_bytes())
            .map_err(|error| Failure::new("merge", error))?;
        Ok(())
    }

    fn restore(&mut self, source: &str, path: &str) -> Result<(), Failure> {
        self.git(&[
            "restore",
            &format!("--source={source}"),
            "--staged",
            "--worktree",
            "--",
            path,
        ])?;
        Ok(())
    }

    fn resolve_lock(&mut self, old: &Lockfile, yard: &Lockfile) -> Result<Vec<String>, Failure> {
        eprintln!("Running cargo metadata --format-version 1");
        let metadata = self.capture("cargo", &["metadata", "--format-version", "1"], "metadata")?;
        if !metadata.success {
            return Err(Failure::new(
                "metadata",
                scrub_urls(String::from_utf8_lossy(&metadata.stderr).trim()),
            ));
        }
        let metadata: Value = serde_json::from_slice(&metadata.stdout)
            .map_err(|error| Failure::new("metadata", error))?;
        let resolved = lockfile(&self.read(&self.root.join("Cargo.lock"), "lockfile")?)?;
        if let Some((pin, replacement)) = moved_pin(yard, &resolved, &metadata) {
            let new = if let Some(replacement) = &replacement {
                replacement.label()
            } else {
                let replacements: Vec<_> = resolved
                    .packages
                    .keys()
                    .filter(|pkg| pkg.name == pin.name)
                    .map(Package::label)
                    .collect();
                if replacements.is_empty() {
                    "removed".into()
                } else {
                    replacements.join(", ")
                }
            };
            let requirements =
                pin_requirements(self.root, &metadata, &pin.name, replacement.as_ref());
            return Err(Failure::new(
                "lock",
                format!(
                    "{} moved from YARD's pin {} to {new}{requirements}",
                    pin.name,
                    pin.label()
                ),
            ));
        }
        let mut changes = Vec::new();
        for (label, baseline) in [("YARD", yard), ("developer", old)] {
            for (pkg, before) in &baseline.packages {
                match resolved.packages.get(pkg) {
                    None => changes.push(format!("Against {label}: removed {}", pkg.label())),
                    Some(after) if before != after => {
                        let keys: BTreeSet<_> = before.keys().chain(after.keys()).collect();
                        for key in keys
                            .into_iter()
                            .filter(|key| before.get(*key) != after.get(*key))
                        {
                            changes.push(format!(
                                "Against {label}: changed {key} for {}",
                                pkg.label()
                            ));
                        }
                    }
                    Some(_) => {}
                }
            }
            for pkg in resolved
                .packages
                .keys()
                .filter(|pkg| !baseline.packages.contains_key(*pkg))
            {
                changes.push(format!("Against {label}: added {}", pkg.label()));
            }
            let keys: BTreeSet<_> = baseline
                .fields
                .keys()
                .chain(resolved.fields.keys())
                .collect();
            for key in keys
                .into_iter()
                .filter(|key| baseline.fields.get(*key) != resolved.fields.get(*key))
            {
                changes.push(format!("Against {label}: changed lockfile {key}"));
            }
        }
        Ok(changes)
    }
}

fn yard_owned(path: &str) -> bool {
    matches!(
        path.split('/').next().unwrap_or_default(),
        "crates"
            | "extensions"
            | "examples"
            | "tooling"
            | "docs"
            | "production.json"
            | "AGENTS.md"
            | "CLAUDE.md"
            | "LICENSE"
            | "README.md"
            | ".cargo"
            | ".github"
            | ".gitignore"
            | ".gitattributes"
    ) || path.starts_with("app/yard/")
}

fn parse_manifest<'a, T: Deserialize<'a>>(text: &'a str) -> Result<T, Failure> {
    toml::from_str(text).map_err(|error| Failure::new("manifest", scrub_urls(&error.to_string())))
}

fn workspace(table: &toml::Table) -> Result<&toml::Table, Failure> {
    table
        .get("workspace")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| Failure::new("manifest", "Cargo.toml has no [workspace] table"))
}

fn entries<'a>(workspace: &'a toml::Table, key: &str) -> Result<Vec<&'a str>, Failure> {
    let invalid = || {
        Failure::new(
            "manifest",
            format!("workspace.{key} must be an array of strings"),
        )
    };
    let Some(value) = workspace.get(key) else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .map(|entry| entry.as_str().ok_or_else(invalid))
        .collect()
}

fn app_member(path: &str) -> bool {
    let mut parts = path.split('/').filter(|part| !matches!(*part, "" | "."));
    parts.next() == Some("app")
        && parts.next().is_some_and(|name| {
            !matches!(name, "yard" | "..") && !name.contains(['*', '?', '[', '\\'])
        })
        && !parts.any(|part| part == "..")
}

fn platform_manifest(mut table: toml::Table) -> Result<toml::Table, Failure> {
    let workspace = table
        .get_mut("workspace")
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| Failure::new("manifest", "Cargo.toml has no [workspace] table"))?;
    workspace.remove("default-members");
    if let Some(toml::Value::Array(items)) = workspace.get_mut("members") {
        items.retain(|entry| !entry.as_str().is_some_and(app_member));
    }
    Ok(table)
}

fn manifest_value(key: &str, value: Option<&toml::Value>) -> String {
    let value = value.map_or_else(|| "<removed>".into(), ToString::to_string);
    scrub_urls(&format!("{key} = {value}"))
}

fn required_yard_keys(
    key: &str,
    base: Option<&toml::Value>,
    original: Option<&toml::Value>,
    yard: Option<&toml::Value>,
    staged: Option<&toml::Value>,
    kept_exclude_resolution: bool,
    missing: &mut Vec<String>,
) {
    if base == yard || yard == staged || (kept_exclude_resolution && key == "workspace.exclude") {
        return;
    }
    // Walk independently added tables too, so a developer's profile.dev
    // does not let a resolution drop YARD's profile.release.
    if [base, original, yard]
        .into_iter()
        .flatten()
        .all(toml::Value::is_table)
    {
        let base_table = base.and_then(toml::Value::as_table);
        let yard_table = yard.and_then(toml::Value::as_table);
        let names: BTreeSet<_> = [base_table, yard_table]
            .into_iter()
            .flatten()
            .flat_map(toml::Table::keys)
            .collect();
        if !names.is_empty() {
            let original_table = original.and_then(toml::Value::as_table);
            let staged_table = staged.and_then(toml::Value::as_table);
            for name in names {
                let child = if key.is_empty() {
                    name.clone()
                } else {
                    format!("{key}.{name}")
                };
                required_yard_keys(
                    &child,
                    base_table.and_then(|table| table.get(name)),
                    original_table.and_then(|table| table.get(name)),
                    yard_table.and_then(|table| table.get(name)),
                    staged_table.and_then(|table| table.get(name)),
                    kept_exclude_resolution,
                    missing,
                );
            }
            return;
        }
    }
    if base == original {
        missing.push(manifest_value(key, yard));
    }
}

fn manifest_differences(
    key: &str,
    base: Option<&toml::Value>,
    developer: Option<&toml::Value>,
    differences: &mut Vec<String>,
) {
    if base == developer {
        return;
    }
    if let (Some(toml::Value::Table(base)), Some(toml::Value::Table(developer))) = (base, developer)
    {
        let keys: BTreeSet<_> = base.keys().chain(developer.keys()).collect();
        for name in keys {
            let key = if key.is_empty() {
                name.clone()
            } else {
                format!("{key}.{name}")
            };
            manifest_differences(&key, base.get(name), developer.get(name), differences);
        }
    } else {
        differences.push(manifest_value(key, developer));
    }
}

fn developer_app_root(root: &Path, member: &Path) -> bool {
    member.components().count() == 2
        && member.to_str().is_some_and(app_member)
        && root.join(member).join("Cargo.toml").is_file()
}

fn developer_app_members(root: &Path, patterns: &[&str]) -> Result<Vec<PathBuf>, Failure> {
    let mut members = expanded(
        root,
        patterns.iter().copied().filter(|member| app_member(member)),
    )?;
    members.extend(
        expanded(root, patterns.iter().copied())?
            .into_iter()
            .filter(|member| developer_app_root(root, member)),
    );
    Ok(members)
}

fn newly_excludes_app_member(
    members: &[PathBuf],
    original_exclusions: &[PathBuf],
    yard_exclusions: &[PathBuf],
) -> bool {
    members.iter().any(|member| {
        !original_exclusions
            .iter()
            .any(|path| member.starts_with(path))
            && yard_exclusions.iter().any(|path| member.starts_with(path))
    })
}

fn merged_manifest(
    root: &Path,
    mut yard_text: String,
    original: &toml::Table,
    yard: &toml::Table,
    manifest_keys: &mut Vec<String>,
) -> Result<String, Failure> {
    let original = workspace(original)?;
    let yard = workspace(yard)?;
    let developer_members = entries(original, "members")?;
    let yard_members = entries(yard, "members")?;
    let defaults = entries(original, "default-members")?;
    let original_exclusions = expanded(root, entries(original, "exclude")?)?;
    let yard_exclusions = expanded(root, entries(yard, "exclude")?)?;
    let mut additions: Vec<String> = developer_members
        .iter()
        .copied()
        .filter(|member| app_member(member) && !yard_members.contains(member))
        .map(str::to_owned)
        .collect();
    let covered = expanded(
        root,
        yard_members
            .iter()
            .copied()
            .chain(additions.iter().map(String::as_str)),
    )?;
    let owned_members = developer_app_members(root, &developer_members)?;
    for member in &owned_members {
        if developer_app_root(root, member)
            && !covered.contains(member)
            && !original_exclusions
                .iter()
                .any(|path| member.starts_with(path))
        {
            additions.push(member.to_string_lossy().replace('\\', "/"));
        }
    }
    if newly_excludes_app_member(&owned_members, &original_exclusions, &yard_exclusions)
        && !manifest_keys
            .iter()
            .any(|key| key.starts_with("workspace.exclude = "))
    {
        manifest_keys.push(scrub_urls(&format!(
            "workspace.exclude = {}",
            yard["exclude"]
        )));
    }
    if !manifest_keys.is_empty() {
        return Ok(yard_text);
    }

    if !original.contains_key("default-members") && yard.contains_key("default-members") {
        remove_default_members(&mut yard_text)?;
    }
    let replace_defaults = original.contains_key("default-members")
        && yard.get("default-members") != original.get("default-members");
    let text = if yard.contains_key("members") {
        let parsed: Manifest = parse_manifest(&yard_text)?;
        edit_manifest(
            yard_text,
            parsed.workspace,
            replace_defaults.then_some(defaults.as_slice()),
            &additions.iter().map(String::as_str).collect::<Vec<_>>(),
        )
    } else if replace_defaults || !additions.is_empty() {
        let mut table = parse_manifest::<toml::Table>(&yard_text)?;
        let workspace = table
            .get_mut("workspace")
            .and_then(toml::Value::as_table_mut)
            .ok_or_else(|| Failure::new("manifest", "Cargo.toml has no [workspace] table"))?;
        if replace_defaults {
            workspace.insert(
                "default-members".into(),
                toml::Value::try_from(&defaults)
                    .map_err(|error| Failure::new("manifest", error))?,
            );
        }
        if !additions.is_empty() {
            workspace.insert("members".into(), toml::Value::from(additions));
        }
        table.to_string()
    } else {
        yard_text
    };
    let table = parse_manifest(&text)?;
    validate_workspace(root, &table)?;
    Ok(text)
}

fn validate_workspace(root: &Path, manifest: &toml::Table) -> Result<(), Failure> {
    let workspace = workspace(manifest)?;
    let exclusions = expanded(root, entries(workspace, "exclude")?)?;
    let mut members = expanded(root, entries(workspace, "members")?)?;
    members.retain(|member| !exclusions.iter().any(|path| member.starts_with(path)));
    if manifest.get("package").is_some_and(toml::Value::is_table) {
        members.push(PathBuf::new());
    }
    for member in &members {
        if !root.join(member).join("Cargo.toml").is_file() {
            return Err(Failure::new(
                "manifest",
                format!("workspace member {} has no Cargo.toml", member.display()),
            ));
        }
    }
    for default in expanded(root, entries(workspace, "default-members")?)? {
        if !members.contains(&default) {
            return Err(Failure::new(
                "manifest",
                format!(
                    "default workspace member {} is not a workspace member",
                    default.display()
                ),
            ));
        }
    }
    Ok(())
}

fn remove_default_members(text: &mut String) -> Result<(), Failure> {
    #[derive(Deserialize)]
    struct ManifestKeys {
        workspace: BTreeMap<toml::Spanned<String>, toml::Spanned<toml::Value>>,
    }

    let manifest: ManifestKeys = parse_manifest(text)?;
    let (key, value) = manifest
        .workspace
        .iter()
        .find(|(key, _)| key.get_ref() == "default-members")
        .ok_or_else(|| Failure::new("manifest", "missing default-members span"))?;
    let start = key.span().start;
    let end = value.span().end;
    let line_start = text[..start].rfind('\n').map_or(0, |pos| pos + 1);
    let line_end = text[end..]
        .find('\n')
        .map_or(text.len(), |pos| end + pos + 1);
    let before = text[line_start..start].trim_end();
    let after = text[end..line_end].trim_start();
    if after.starts_with(',') {
        let comma = end
            + text[end..line_end]
                .find(',')
                .expect("comma follows the value");
        text.replace_range(start..comma + 1, "");
    } else if before.ends_with(',') {
        text.replace_range(line_start + before.len() - 1..end, "");
    } else if before.ends_with('{') {
        text.replace_range(start..end, "");
    } else {
        text.replace_range(line_start..line_end, "");
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct Package {
    name: String,
    version: String,
    source: Option<String>,
}

impl Package {
    fn matches_lock_dependency(&self, dependency: &str) -> bool {
        let mut parts = dependency.splitn(3, ' ');
        parts.next() == Some(self.name.as_str())
            && parts.next().is_none_or(|version| version == self.version)
            && parts.next().is_none_or(|source| {
                self.source.as_deref().is_some_and(|value| {
                    source
                        .strip_prefix('(')
                        .and_then(|source| source.strip_suffix(')'))
                        == Some(value)
                })
            })
    }

    fn label(&self) -> String {
        match &self.source {
            Some(source) => {
                let revision = source.rsplit_once('#').map_or("", |(_, revision)| revision);
                let source = safe_remote(source);
                let revision = if revision.bytes().all(|byte| byte.is_ascii_hexdigit())
                    && !revision.is_empty()
                    && !source.contains('#')
                {
                    format!("#{revision}")
                } else {
                    String::new()
                };
                format!("{} {} ({source}{revision})", self.name, self.version)
            }
            None => format!("{} {}", self.name, self.version),
        }
    }
}

struct Lockfile {
    packages: BTreeMap<Package, toml::Table>,
    fields: toml::Table,
}

fn json_items(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

fn moved_pin<'a>(
    yard: &'a Lockfile,
    resolved: &Lockfile,
    metadata: &Value,
) -> Option<(&'a Package, Option<Package>)> {
    let packages: BTreeMap<_, _> = json_items(&metadata["packages"])
        .iter()
        .filter_map(|package| Some((package["id"].as_str()?, package)))
        .collect();
    for node in json_items(&metadata["resolve"]["nodes"]) {
        let Some(owner) = node["id"].as_str().and_then(|id| packages.get(id)) else {
            continue;
        };
        let Some(owner) = metadata_package(owner) else {
            continue;
        };
        let Some(table) = yard.packages.get(&owner) else {
            continue;
        };
        let Some(dependencies) = table.get("dependencies").and_then(toml::Value::as_array) else {
            continue;
        };
        let selected: Vec<_> = json_items(&node["dependencies"])
            .iter()
            .filter_map(Value::as_str)
            .filter_map(|id| packages.get(id))
            .filter_map(|package| metadata_package(package))
            .collect();
        for dependency in dependencies.iter().filter_map(toml::Value::as_str) {
            let Some(pin) = yard
                .packages
                .keys()
                .find(|pin| pin.matches_lock_dependency(dependency))
            else {
                continue;
            };
            if !selected.contains(pin)
                && let Some(replacement) = selected.iter().find(|package| package.name == pin.name)
            {
                return Some((pin, Some(replacement.clone())));
            }
        }
    }
    yard.packages
        .keys()
        .find(|pin| !resolved.packages.contains_key(*pin))
        .map(|pin| (pin, None))
}

fn metadata_package(package: &Value) -> Option<Package> {
    Some(Package {
        name: package["name"].as_str()?.into(),
        version: package["version"].as_str()?.into(),
        source: package["source"].as_str().map(str::to_owned),
    })
}

fn pin_requirements(
    root: &Path,
    metadata: &Value,
    name: &str,
    replacement: Option<&Package>,
) -> String {
    let packages = json_items(&metadata["packages"]);
    let replacement_id = replacement.and_then(|replacement| {
        packages.iter().find_map(|package| {
            if metadata_package(package).as_ref() == Some(replacement) {
                package["id"].as_str()
            } else {
                None
            }
        })
    });
    let nodes = json_items(&metadata["resolve"]["nodes"]);
    let root = metadata["workspace_root"].as_str().map_or(root, Path::new);
    let mut best_rank = 4;
    let mut requirements = BTreeSet::new();
    for package in packages {
        let is_member = json_items(&metadata["workspace_members"]).contains(&package["id"]);
        let is_app = package["manifest_path"].as_str().is_some_and(|path| {
            let path = Path::new(path);
            path.starts_with(root.join("app")) && !path.starts_with(root.join("app/yard"))
        });
        let rank = match (is_member, is_app) {
            (true, true) => {
                let depends_on_replacement = replacement_id.is_some_and(|id| {
                    nodes.iter().any(|node| {
                        node["id"] == package["id"]
                            && json_items(&node["dependencies"])
                                .iter()
                                .any(|dependency| dependency.as_str() == Some(id))
                    })
                });
                usize::from(!depends_on_replacement)
            }
            (true, false) => 2,
            (false, _) => 3,
        };
        if rank > best_rank {
            continue;
        }
        for dependency in json_items(&package["dependencies"]) {
            if dependency["name"].as_str() == Some(name)
                && let (Some(owner), Some(requirement)) =
                    (package["name"].as_str(), dependency["req"].as_str())
            {
                if rank < best_rank {
                    requirements.clear();
                    best_rank = rank;
                }
                requirements.insert(format!("{owner} ({requirement})"));
            }
        }
    }
    if requirements.is_empty() {
        String::new()
    } else {
        format!(
            "; required by {}",
            requirements.into_iter().collect::<Vec<_>>().join(", ")
        )
    }
}

fn lockfile(text: &str) -> Result<Lockfile, Failure> {
    let invalid = |error: toml::de::Error| Failure::new("lockfile", error);
    let mut fields: toml::Table = toml::from_str(text).map_err(invalid)?;
    let mut packages = BTreeMap::new();
    if let Some(entries) = fields.remove("package") {
        for entry in entries.try_into::<Vec<toml::Table>>().map_err(invalid)? {
            let package = toml::Value::Table(entry.clone())
                .try_into()
                .map_err(invalid)?;
            packages.insert(package, entry);
        }
    }
    Ok(Lockfile { packages, fields })
}
