//! `cargo xtask bundle macos`: makes `<name>.app`, and with `--zip`
//! `<name>-<version>.zip`, from a product config; signs it with the
//! developer's identity, notarizes it, and wraps it in a disk image when the
//! config's `[release.macos]` asks. `cargo xtask bundle linux` is in
//! [`linux`], and `cargo xtask bundle windows` in [`windows`]; what they all
//! do is here.

mod linux;
mod signing;
mod windows;
pub use linux::{LinuxArgs, LinuxBundled, bundle_linux};
pub use signing::{HELPER_VARIABLES, Secrets};
pub use windows::{
    WindowsArgs, WindowsBundled, WindowsHost, WindowsSecrets, bundle_windows, check_authenticode,
    inno_script, sign_windows_file,
};

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, Write as _};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::process::{Command, ExitCode, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::new_app::{NewAppArgs, new_app};

use clap::{Parser, Subcommand};
use serde_json::Value;
use signing::{SECRET_VARIABLES, Session, Signing};
use yard_build_config::BuildConfig;
use yard_product::Product;

/// The sample application's production config, which its review bundle must differ from.
const PRODUCTION_CONFIG: &str = "app/yard/product.toml";
/// The default `--out`, relative to the repository.
const DEFAULT_OUT: &str = "dist";

#[derive(Parser)]
#[command(name = "cargo xtask")]
struct Args {
    #[command(subcommand)]
    task: Task,
}

#[derive(Subcommand)]
enum Task {
    /// Packages a workspace application for installation.
    #[command(subcommand)]
    Bundle(System),
    /// Creates a workspace application in application/.
    NewApp(NewAppArgs),
    /// What Inno Setup runs to sign the installer and the uninstaller of
    /// `bundle windows`, from what that run gave it.
    #[command(hide = true)]
    SignWindowsFile { file: PathBuf },
}

#[derive(Subcommand)]
enum System {
    /// Makes `<name>.app` from production code, signed ad hoc.
    Macos(MacosArgs),
    /// Makes `<name>-<version>-linux-<arch>.tar.gz` from production code, on
    /// Linux.
    Linux(LinuxArgs),
    /// Makes `<Name>-<version>-<arch>-setup.exe` with Inno Setup, and
    /// `<Name>-<version>-windows-<arch>.zip`, from production code, on
    /// Windows.
    Windows(WindowsArgs),
}

/// The options of `cargo xtask bundle macos`.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct MacosArgs {
    /// The application package [default: the workspace's one default member].
    #[arg(long, value_name = "NAME")]
    pub package: Option<String>,
    /// The product config [default: product.toml in the application package].
    #[arg(long, value_name = "FILE")]
    pub config: Option<PathBuf>,
    /// The folder the bundle goes into [default: dist in the repository].
    #[arg(long, value_name = "FOLDER")]
    pub out: Option<PathBuf>,
    /// Builds a review bundle, which runs `dev`, with a product other than
    /// the production one.
    #[arg(long)]
    pub dev: bool,
    /// An environment variable of the review bundle (`LSEnvironment`).
    /// Needs `--dev`.
    #[arg(long = "env", value_name = "KEY=VALUE")]
    pub env: Vec<String>,
    /// Also writes `<out>/<name>-<version>.zip`, a release, and prints its
    /// sha256.
    #[arg(long)]
    pub zip: bool,
}

/// What `bundle_macos` made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundled {
    pub app: PathBuf,
    /// The zip and its SHA-256 in hex, with `--zip`.
    pub zip: Option<(PathBuf, String)>,
    /// The disk image and its SHA-256 in hex, with `dmg = true`.
    pub dmg: Option<(PathBuf, String)>,
}

/// The reason of a run that stopped because it was interrupted.
const INTERRUPTED: &str = "interrupted";

/// Why `bundle_macos` stopped: the step, and the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub step: &'static str,
    pub reason: String,
}

impl Failure {
    fn new(step: &'static str, reason: impl fmt::Display) -> Self {
        Self {
            step,
            reason: reason.to_string(),
        }
    }

    /// Whether this step can refuse input. Check for an interrupt first.
    pub fn is_refusal(&self) -> bool {
        matches!(
            self.step,
            "arguments" | "package" | "config" | "secrets" | "tools"
        )
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.step, self.reason)
    }
}

/// Whether the run was asked to stop: set on SIGINT, SIGTERM, or SIGHUP by
/// the handler [`main`] installs, and read before each step, so the run stops
/// after the current step and still cleans up. Tests set it without a
/// signal. A second signal does not kill xtask either; in a terminal, Ctrl-C
/// also reaches the running child, which stops.
#[derive(Debug, Clone, Default)]
pub struct Interrupt(Arc<AtomicBool>);

impl Interrupt {
    /// Asks the run to stop. The run logs it when it sees it, inside its
    /// group.
    pub fn set(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// The exit code of a run that did not succeed: 130 when it was
    /// interrupted, as a shell reports Ctrl-C, else 1.
    pub fn exit_code(&self) -> u8 {
        if self.is_set() { 130 } else { 1 }
    }
}

/// Runs the external commands, so tests can check them without running them.
pub trait Runner {
    /// Runs `command` to its end, its output on stderr.
    fn run(&mut self, command: &mut Command) -> Result<(), String>;
    /// Runs `command` to its end and returns what it wrote on stdout; its
    /// stderr stays on stderr.
    fn output(&mut self, command: &mut Command) -> Result<Vec<u8>, String>;
    /// Runs `command` to its end and returns whether it succeeded and what
    /// it wrote on stdout, which a failed command may still have written
    /// (such as `notarytool`'s report of a rejection); its stderr stays on
    /// stderr.
    fn report(&mut self, command: &mut Command) -> Result<(bool, Vec<u8>), String>;
    /// Runs `command` with `input` on its stdin and returns what it wrote on
    /// stdout. The input may hold a secret, and is never logged.
    fn input(&mut self, command: &mut Command, input: &[u8]) -> Result<Vec<u8>, String>;
    /// Runs `command` to its end and returns what it wrote on stdout and
    /// stderr, neither printed, so the caller can hide a secret in them
    /// first. `Err` only when it could not run.
    fn capture(&mut self, command: &mut Command) -> Result<Captured, String>;
}

/// What [`Runner::capture`] returns.
#[derive(Debug, Clone, Default)]
pub struct Captured {
    pub success: bool,
    /// How it ended, such as `exit code: 1`.
    pub status: String,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Runs the commands for real. A command that a SIGINT, SIGTERM, or SIGHUP
/// ended sets `interrupt`, as the same signal reaches xtask's handler, so
/// the run is an interrupt whichever of the two is seen first.
pub struct SystemRunner {
    pub interrupt: Interrupt,
}

impl SystemRunner {
    /// Whether the command succeeded; when a signal of an interrupt ended
    /// it, sets the interrupt.
    fn succeeded(&self, command: &Command, status: ExitStatus) -> Result<(), String> {
        if status.success() {
            return Ok(());
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt as _;
            // SIGHUP, SIGINT, and SIGTERM, the same on every Unix.
            if let Some(signal) = status.signal()
                && [1, 2, 15].contains(&signal)
            {
                self.interrupt.set();
            }
        }
        Err(format!("{} exited with {status}", program(command)))
    }
}

impl Runner for SystemRunner {
    fn run(&mut self, command: &mut Command) -> Result<(), String> {
        let status = command
            .stdout(Stdio::from(io::stderr()))
            .status()
            .map_err(|error| could_not_run(command, error))?;
        self.succeeded(command, status)
    }

    fn output(&mut self, command: &mut Command) -> Result<Vec<u8>, String> {
        let output = command
            .stderr(Stdio::inherit())
            .output()
            .map_err(|error| could_not_run(command, error))?;
        self.succeeded(command, output.status)?;
        Ok(output.stdout)
    }

    fn report(&mut self, command: &mut Command) -> Result<(bool, Vec<u8>), String> {
        let output = command
            .stderr(Stdio::inherit())
            .output()
            .map_err(|error| could_not_run(command, error))?;
        // A failure is reported, not returned, but one ended by an
        // interrupt still sets it.
        let _ = self.succeeded(command, output.status);
        Ok((output.status.success(), output.stdout))
    }

    fn capture(&mut self, command: &mut Command) -> Result<Captured, String> {
        let output = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| could_not_run(command, error))?;
        // A failure is returned as data, but one ended by an interrupt
        // still sets it.
        let _ = self.succeeded(command, output.status);
        Ok(Captured {
            success: output.status.success(),
            status: output.status.to_string(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    fn input(&mut self, command: &mut Command, input: &[u8]) -> Result<Vec<u8>, String> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| could_not_run(command, error))?;
        let written = child.stdin.take().expect("stdin is piped").write_all(input);
        let output = child
            .wait_with_output()
            .map_err(|error| could_not_run(command, error))?;
        self.succeeded(command, output.status)?;
        written.map_err(|error| format!("could not write to {}: {error}", program(command)))?;
        Ok(output.stdout)
    }
}

fn program(command: &Command) -> String {
    command.get_program().to_string_lossy().into_owned()
}

fn could_not_run(command: &Command, error: io::Error) -> String {
    format!("could not run {}: {error}", program(command))
}

/// The repository's root, where the cargo commands run and the defaults are.
pub fn repository() -> PathBuf {
    // `tooling/xtask` is two folders below the root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("xtask is in tooling/xtask")
        .to_owned()
}

/// Where xtask's own build output goes: `CARGO_TARGET_DIR` when it is set,
/// else `target` in the repository. xtask's logs are kept there, never in
/// the `--out` folder.
pub fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| repository().join("target"))
}

/// The command line: starts both logs of `yard_log` as the product `xtask`
/// (`xtask.log` and `lifecycle.jsonl` in [`target_dir`]), parses the
/// arguments, runs the task, prints the result, and exits 1 on a failure, or
/// 130 when it was interrupted. A signal stops a bundle after the current
/// step, so the keychain is always cleaned up.
pub fn main() -> ExitCode {
    yard_log::start(Product::new(
        "xtask",
        env!("CARGO_PKG_VERSION"),
        "dev.yard.xtask",
    ));
    let args = Args::parse();
    if let Task::SignWindowsFile { file } = &args.task {
        // The bundle that runs this through Inno Setup writes xtask's log
        // files meanwhile, so this logs to stderr only.
        // It ends through `yard_log::exit`, so its trace reaches stderr.
        match sign_windows_file(
            file,
            &|name| std::env::var_os(name),
            &mut SystemRunner {
                interrupt: Interrupt::default(),
            },
        ) {
            Ok(()) => yard_log::exit(0),
            Err(reason) => {
                eprintln!("sign-windows-file: {reason}");
                yard_log::exit(1)
            }
        }
    }
    yard_log::open_files(&target_dir());
    // xtask has no startup of its own: each task is a group of its own.
    yard_log::startup_finished();
    let interrupt = Interrupt::default();
    let handler = interrupt.clone();
    if matches!(args.task, Task::Bundle(_))
        && let Err(error) = ctrlc::set_handler(move || handler.set())
    {
        log::warn!("could not handle interrupts: {error}");
    }
    match args.task {
        Task::NewApp(options) => match new_app(&repository(), &options) {
            Ok(made) => {
                println!(
                    "Created {} (package {})",
                    made.folder.display(),
                    made.package
                );
                println!("Run: cargo run");
                println!("Test: cargo test");
                println!("Bundle: cargo xtask bundle <macos|linux|windows>");
                if made.placeholder_icon {
                    println!("Using the placeholder icon in application/icon.png.");
                }
                yard_log::exit(0)
            }
            Err(failure) => {
                eprintln!("new-app: {failure}");
                yard_log::exit(1)
            }
        },
        Task::Bundle(System::Macos(options)) => {
            match bundle_macos(
                &options,
                &Secrets::from_env(),
                &interrupt,
                &mut SystemRunner {
                    interrupt: interrupt.clone(),
                },
            ) {
                Ok(bundled) => {
                    if let Some((_, digest)) = &bundled.zip {
                        println!("sha256 {digest}");
                    }
                    if let Some((_, digest)) = &bundled.dmg {
                        println!("dmg-sha256 {digest}");
                    }
                    println!("{}", bundled.app.display());
                    ExitCode::SUCCESS
                }
                Err(failure) => {
                    eprintln!("{failure}");
                    ExitCode::from(interrupt.exit_code())
                }
            }
        }
        Task::Bundle(System::Linux(options)) => {
            match bundle_linux(
                &options,
                std::env::consts::OS,
                &interrupt,
                &mut SystemRunner {
                    interrupt: interrupt.clone(),
                },
            ) {
                Ok(bundled) => {
                    let (archive, digest) = &bundled.archive;
                    println!("sha256 {digest}");
                    println!("{}", archive.display());
                    ExitCode::SUCCESS
                }
                Err(failure) => {
                    eprintln!("{failure}");
                    ExitCode::from(interrupt.exit_code())
                }
            }
        }
        Task::SignWindowsFile { .. } => unreachable!("signed above"),
        Task::Bundle(System::Windows(options)) => {
            match bundle_windows(
                &options,
                &WindowsHost::from_env(),
                &WindowsSecrets::from_env(),
                &interrupt,
                &mut SystemRunner {
                    interrupt: interrupt.clone(),
                },
            ) {
                Ok(bundled) => {
                    for (path, digest) in [&bundled.installer, &bundled.zip] {
                        println!("sha256 {digest}");
                        println!("{}", path.display());
                    }
                    ExitCode::SUCCESS
                }
                Err(failure) => {
                    eprintln!("{failure}");
                    ExitCode::from(interrupt.exit_code())
                }
            }
        }
    }
}

/// Packages the selected application as `<out>/<name>.app`, inside the group
/// `bundle-macos`. The group includes `config` when supplied, and the
/// `package` debug event reports the resolved target and config. Build and
/// packaging steps are printed on stdout. A refusal is logged at `info`,
/// a failure at `error` in the diagnostic log.
/// The secrets come from the environment through [`Secrets::from_env`].
/// Once `interrupt` is set, the run stops before its next step, and before
/// it publishes anything, with the reason `interrupted`. A run is an
/// interrupt when `interrupt` is set once its clean-up is done, even if
/// every step succeeded or a step failed of the same signal; it is logged at
/// `info`.
pub fn bundle_macos(
    options: &MacosArgs,
    secrets: &Secrets,
    interrupt: &Interrupt,
    runner: &mut dyn Runner,
) -> Result<Bundled, Failure> {
    let _group = tracing::info_span!(
        "bundle-macos",
        config = options
            .config
            .as_deref()
            .map(|path| tracing::field::display(path.display())),
        dev = options.dev,
        zip = options.zip
    )
    .entered();
    let result = bundle(options, secrets, interrupt, runner);
    log_outcome(&result, interrupt);
    result
}

/// Logs how a run ended inside its group. Interrupts take precedence.
/// Interrupts and refusals log at `info`, other failures at `error`.
pub(super) fn log_outcome<T>(result: &Result<T, Failure>, interrupt: &Interrupt) {
    if let Err(failure) = result {
        if interrupt.is_set() {
            tracing::info!(
                step = failure.step,
                reason = %failure.reason,
                "stopped: interrupted"
            );
        } else if failure.is_refusal() {
            tracing::info!(step = failure.step, "refused: {}", failure.reason);
        } else {
            log::error!("failed at {}: {}", failure.step, failure.reason);
        }
    }
}

/// Checks the arguments every system shares, and makes the config and the
/// `--out` folder absolute.
fn arguments(
    config: Option<&Path>,
    out: Option<&Path>,
    dev: bool,
    env: &[String],
) -> Result<(Option<PathBuf>, PathBuf), Failure> {
    let arguments = |reason: String| Failure::new("arguments", reason);
    env_pairs(env).map_err(arguments)?;
    if !dev && !env.is_empty() {
        return Err(arguments("--env requires --dev".into()));
    }
    let config = config
        .map(std::path::absolute)
        .transpose()
        .map_err(|error| arguments(error.to_string()))?;
    let out = out
        .map(Path::to_owned)
        .unwrap_or_else(|| repository().join(DEFAULT_OUT));
    let out = std::path::absolute(out).map_err(|error| arguments(error.to_string()))?;
    Ok((config, out))
}

/// The workspace package to bundle, its one binary, and its manifest directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub package: String,
    pub binary: String,
    pub folder: PathBuf,
}

/// Selects an application from `cargo metadata --no-deps --format-version 1` JSON.
pub fn bundle_target(metadata: &[u8], package: Option<&str>, dev: bool) -> Result<Target, String> {
    let metadata: Value = serde_json::from_slice(metadata)
        .map_err(|error| format!("invalid cargo metadata: {error}"))?;
    select_target(&metadata, package, dev)
}

fn select_target(metadata: &Value, package: Option<&str>, dev: bool) -> Result<Target, String> {
    let packages = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata has no packages list")?;
    let members = metadata["workspace_members"]
        .as_array()
        .ok_or("cargo metadata has no workspace members list")?;
    let package = match package {
        Some(name) => packages
            .iter()
            .find(|package| package["name"] == name && members.contains(&package["id"]))
            .ok_or_else(|| format!("{name} is not a workspace package"))?,
        None => {
            let Some([id]) = metadata["workspace_default_members"]
                .as_array()
                .map(Vec::as_slice)
            else {
                return Err("expected one default workspace member; specify --package".into());
            };
            packages
                .iter()
                .find(|package| package["id"] == *id && members.contains(id))
                .ok_or("default workspace member was not found; specify --package")?
        }
    };
    let name = package["name"]
        .as_str()
        .ok_or("workspace package has no name")?;
    if dev && name != "yard" {
        return Err(format!("--dev is only supported by yard; selected {name}"));
    }

    let mut binaries = package["targets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|target| is_binary(target));
    let binary = binaries
        .next()
        .and_then(|target| target["name"].as_str())
        .filter(|_| binaries.next().is_none())
        .ok_or_else(|| format!("{name} must have exactly one binary target"))?;
    let folder = package["manifest_path"]
        .as_str()
        .and_then(|manifest| Path::new(manifest).parent())
        .ok_or_else(|| format!("{name} has no manifest directory"))?;
    Ok(Target {
        package: name.to_owned(),
        binary: binary.to_owned(),
        folder: folder.to_owned(),
    })
}

fn is_binary(target: &Value) -> bool {
    target["kind"]
        .as_array()
        .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
}

/// Resolves and logs the bundle target and the config it will use.
fn resolve_package(
    package: Option<&str>,
    dev: bool,
    config: Option<PathBuf>,
    removed: &[OsString],
    interrupt: &Interrupt,
    runner: &mut dyn Runner,
) -> Result<(Target, PathBuf), Failure> {
    not_interrupted("package", interrupt)?;
    let metadata = runner
        .output(cargo(removed).args(["metadata", "--no-deps", "--format-version", "1"]))
        .map_err(|reason| Failure::new("metadata", reason))?;
    let metadata = serde_json::from_slice(&metadata)
        .map_err(|error| Failure::new("metadata", format!("invalid cargo metadata: {error}")))?;
    let target =
        select_target(&metadata, package, dev).map_err(|reason| Failure::new("package", reason))?;
    let config = config.unwrap_or_else(|| target.folder.join("product.toml"));
    tracing::debug!(
        package = %target.package,
        binary = %target.binary,
        config = %config.display(),
        "package"
    );
    Ok((target, config))
}

/// Runs YARD's stages check, then builds the selected binary in release mode
/// with `PRODUCT_CONFIG` and `envs`. Enables default features for `dev`
/// bundles. Returns the executable Cargo reports.
fn build(
    target: &Target,
    config: &Path,
    dev: bool,
    envs: &[(&str, &str)],
    removed: &[OsString],
    interrupt: &Interrupt,
    runner: &mut dyn Runner,
) -> Result<PathBuf, Failure> {
    step("stages", interrupt)?;
    runner
        .run(
            cargo(removed)
                .args(["test", "-p", "yard", "--test", "stages"])
                .env_remove("YARD_PROMOTE"),
        )
        .map_err(|reason| Failure::new("stages", reason))?;

    step("build", interrupt)?;
    let mut build = cargo(removed);
    build
        .args([
            "build",
            "--release",
            "-p",
            &target.package,
            "--bin",
            &target.binary,
        ])
        .env("PRODUCT_CONFIG", config)
        .envs(envs.iter().copied());
    if !dev {
        build.arg("--no-default-features");
    }
    build.arg("--message-format=json-render-diagnostics");
    let report = runner
        .output(&mut build)
        .map_err(|reason| Failure::new("build", reason))?;
    let binary = reported_executable(&report, &target.binary)
        .map_err(|reason| Failure::new("build", reason))?;
    tracing::debug!(binary = %binary.display(), "built");
    Ok(binary)
}

/// A run that succeeded is still an interrupt when a signal arrived during
/// its last step or its clean-up.
fn interrupted_at_end<T>(result: Result<T, Failure>, interrupt: &Interrupt) -> Result<T, Failure> {
    match result {
        Ok(_) if interrupt.is_set() => {
            tracing::info!("interrupted; stopping after the current step");
            Err(Failure::new("clean-up", INTERRUPTED))
        }
        result => result,
    }
}

fn bundle(
    options: &MacosArgs,
    secrets: &Secrets,
    interrupt: &Interrupt,
    runner: &mut dyn Runner,
) -> Result<Bundled, Failure> {
    tracing::debug!("arguments");
    let arch = Arch::host()?;
    let (config, out) = arguments(
        options.config.as_deref(),
        options.out.as_deref(),
        options.dev,
        &options.env,
    )?;
    let (target, config) = resolve_package(
        options.package.as_deref(),
        options.dev,
        config,
        &[],
        interrupt,
        runner,
    )?;

    tracing::debug!("config");
    let product = read_config(&config, options, arch)?;

    tracing::debug!("secrets");
    let signing =
        Signing::new(&product, secrets).map_err(|reason| Failure::new("secrets", reason))?;

    let binary = build(&target, &config, options.dev, &[], &[], interrupt, runner)?;

    let mut session = Session::default();
    let job = Job {
        product: &product,
        options,
        binary: &binary,
        out: &out,
        signing: &signing,
        interrupt,
    };
    // A panic, too, leaves no keychain behind: the clean-up runs, then the
    // panic goes on.
    let result = panic::catch_unwind(AssertUnwindSafe(|| signed(&job, &mut session, runner)));
    if signing.identity().is_some() {
        tracing::debug!("keychain clean-up");
    }
    let cleaned = session.clean_up(runner);
    let result = result.unwrap_or_else(|panic| panic::resume_unwind(panic));
    let result = match (result, cleaned) {
        (Ok(bundled), Ok(())) => Ok(bundled),
        (Err(failure), Ok(())) => Err(failure),
        (Err(mut failure), Err(reason)) => {
            failure.reason = format!("{}; then {reason}", failure.reason);
            Err(failure)
        }
        (Ok(_), Err(reason)) => Err(Failure::new("keychain", reason)),
    };
    interrupted_at_end(result, interrupt)
}

/// What one run bundles, and how.
struct Job<'a> {
    product: &'a BuildConfig,
    options: &'a MacosArgs,
    binary: &'a Path,
    out: &'a Path,
    signing: &'a Signing,
    interrupt: &'a Interrupt,
}

/// The steps from the keychain on, inside the session that
/// [`Session::clean_up`] undoes.
fn signed(job: &Job, session: &mut Session, runner: &mut dyn Runner) -> Result<Bundled, Failure> {
    let Job { out, interrupt, .. } = *job;
    if let Some(identity) = job.signing.identity() {
        step("keychain", interrupt)?;
        session
            .set_up(identity, runner)
            .map_err(|reason| Failure::new("keychain", reason))?;
    }

    step("icon", interrupt)?;
    staged(out, "icon", "macos", |staging| {
        in_temp(staging, job, session, runner)
    })
}

/// Runs `work` in a new [`Staging`] folder of `out`, made at the step
/// `step`, and closes it: what `work` published stays, or, when it fails,
/// every previous file is put back.
fn staged<T>(
    out: &Path,
    step: &'static str,
    system: &str,
    work: impl FnOnce(&mut Staging) -> Result<T, Failure>,
) -> Result<T, Failure> {
    fs::create_dir_all(out).map_err(|error| {
        Failure::new(step, format!("could not create {}: {error}", out.display()))
    })?;
    let mut staging = Staging::new(out, system).map_err(|error| {
        Failure::new(
            step,
            format!(
                "could not create a temporary folder in {}: {error}",
                out.display()
            ),
        )
    })?;
    match work(&mut staging) {
        Ok(made) => {
            staging.close(true);
            Ok(made)
        }
        Err(mut failure) => {
            for previous in staging.close(false) {
                failure.reason = format!(
                    "{}; previous file retained at {}",
                    failure.reason,
                    previous.display()
                );
            }
            Err(failure)
        }
    }
}

/// Prints a step's name and logs it, unless the run was interrupted.
fn step(name: &'static str, interrupt: &Interrupt) -> Result<(), Failure> {
    not_interrupted(name, interrupt)?;
    writeln!(io::stdout(), "{name}")
        .map_err(|error| Failure::new(name, format!("could not print the step: {error}")))?;
    tracing::debug!("{name}");
    Ok(())
}

/// Stops the run at `step` once it was interrupted.
fn not_interrupted(step: &'static str, interrupt: &Interrupt) -> Result<(), Failure> {
    if interrupt.is_set() {
        tracing::info!("interrupted; stopping after the current step");
        return Err(Failure::new(step, INTERRUPTED));
    }
    Ok(())
}

/// Runs Cargo in the repository without signing secrets or helper variables.
fn cargo(removed: &[OsString]) -> Command {
    let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.current_dir(repository());
    for name in SECRET_VARIABLES {
        command.env_remove(name);
    }
    for name in removed {
        command.env_remove(name);
    }
    command
}

fn read_config(config: &Path, options: &MacosArgs, arch: Arch) -> Result<BuildConfig, Failure> {
    let product = read_product(config, options.dev, false)?;
    package_checks(config, &product, PackageSystem::Macos, arch)
        .map_err(|reason| Failure::new("config", reason))?;
    Ok(product)
}

/// The system whose package names are checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageSystem {
    Macos,
    Linux,
    Windows,
}

/// An architecture YARD packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
}

impl Arch {
    fn host() -> Result<Self, Failure> {
        match std::env::consts::ARCH {
            "x86_64" => Ok(Self::X86_64),
            "aarch64" => Ok(Self::Aarch64),
            arch => Err(Failure::new(
                "arguments",
                format!("unsupported packaging architecture: {arch}"),
            )),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
        }
    }
}

/// Checks the product for every output the system can make, before building
/// and without depending on tools from the target system.
pub fn package_checks(
    file: &Path,
    product: &BuildConfig,
    system: PackageSystem,
    arch: Arch,
) -> Result<(), String> {
    let file = file.display();
    let arch = arch.name();
    if system == PackageSystem::Windows {
        return windows_package_checks(file, product, arch);
    }

    if !is_path_component(&product.name) {
        return Err(format!("{file}: name must be a single path component"));
    }
    if !is_path_component(&product.version) {
        return Err(format!("{file}: version must be a single path component"));
    }
    let name = &product.name;
    let version = &product.version;
    let names = match system {
        PackageSystem::Macos => vec![
            format!("{name}.app"),
            format!("{name}-{version}.zip"),
            format!("{name}-{version}.dmg"),
        ],
        PackageSystem::Linux => {
            let name = yard_product::file_name(name);
            if name.is_empty() {
                return Err(format!("{file}: name gives no file name for Linux"));
            }
            for extension in ["desktop", "png"] {
                let made = format!("{}.{extension}", product.identifier);
                if made.len() > FILE_NAME_LIMIT {
                    return Err(format!(
                        "{file}: `identifier` makes the file name {made:?}, longer than {FILE_NAME_LIMIT} bytes on Linux"
                    ));
                }
            }
            vec![
                format!("{name}.app"),
                linux_archive_name(&name, version, arch),
            ]
        }
        PackageSystem::Windows => unreachable!(),
    };
    for made in names {
        let len = match system {
            PackageSystem::Macos => made.encode_utf16().count(),
            _ => made.len(),
        };
        if len > FILE_NAME_LIMIT {
            return Err(format!(
                "{file}: name and version make the file name {made:?}, longer than {FILE_NAME_LIMIT} on {system:?}"
            ));
        }
    }
    Ok(())
}

fn linux_archive_name(name: &str, version: &str, arch: &str) -> String {
    format!("{name}-{version}-linux-{arch}.tar.gz")
}

fn setup_base_name(product: &BuildConfig, arch: &str) -> String {
    format!("{}-{}-{arch}-setup", product.name, product.version)
}

fn setup_file_name(product: &BuildConfig, arch: &str) -> String {
    format!("{}.exe", setup_base_name(product, arch))
}

fn windows_zip_name(product: &BuildConfig, arch: &str) -> String {
    format!("{}-{}-windows-{arch}.zip", product.name, product.version)
}

fn windows_package_checks(
    file: impl fmt::Display,
    product: &BuildConfig,
    arch: &str,
) -> Result<(), String> {
    const APP_ID_LIMIT: usize = 127;
    for (key, value) in [("name", &product.name), ("version", &product.version)] {
        if value.contains(['{', '}']) {
            return Err(format!(
                "{file}: `{key}` must not hold `{{` or `}}` on Windows: Inno Setup's preprocessor and constants would read them"
            ));
        }
        if !is_windows_file_name(value) {
            return Err(format!(
                "{file}: `{key}` must be a Windows file name: none of < > : \" / \\ | ? * or a control character, no space at either end, no dot at the end, and not a device name such as CON or COM1"
            ));
        }
    }
    if product.name.starts_with(['@', '-']) {
        return Err(format!(
            "{file}: `name` must not start with `@` or `-` on Windows: tar would read it as an option or an archive to include"
        ));
    }
    let name = &product.name;
    for made in [
        format!("{name}.exe"),
        format!("{name}.lnk"),
        setup_file_name(product, arch),
        windows_zip_name(product, arch),
    ] {
        if made.encode_utf16().count() > FILE_NAME_LIMIT {
            return Err(format!(
                "{file}: `name` and `version` make the file name {made:?}, longer than Windows' {FILE_NAME_LIMIT} characters"
            ));
        }
    }
    if product.identifier.len() > APP_ID_LIMIT {
        return Err(format!(
            "{file}: `identifier` is the installer's AppId, which Inno Setup limits to {APP_ID_LIMIT} characters"
        ));
    }
    Ok(())
}

/// The maximum length of a package file name in the system's units.
const FILE_NAME_LIMIT: usize = 255;

/// Whether Windows accepts `value` as a file name, and keeps it as written.
/// The device names are reserved with any extension, and `COM` and `LPT`
/// with a superscript digit too.
fn is_windows_file_name(value: &str) -> bool {
    const DEVICES: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];
    // Windows drops the spaces at the end of the stem, so `CON .txt` is CON.
    let stem = value
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches([' ', '.'])
        .to_uppercase();
    let numbered = ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|rest| {
            let mut digits = rest.chars();
            digits
                .next()
                .is_some_and(|c| c.is_ascii_digit() || "¹²³".contains(c))
                && digits.next().is_none()
        })
    });
    let device = DEVICES.contains(&stem.as_str()) || numbered;
    !value.is_empty()
        && !device
        && !value.starts_with(' ')
        && !value.ends_with([' ', '.'])
        && !value
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
}

/// Reads the config through `yard_build_config`; with `dev`, refuses a
/// product that takes the production product's name, identifier, or URL
/// scheme, compared without case, and with `linux` also its Linux `<name>`.
fn read_product(config: &Path, dev: bool, linux: bool) -> Result<BuildConfig, Failure> {
    let refuse = |reason: String| Failure::new("config", reason);
    let product = yard_build_config::read(config).map_err(refuse)?;
    if dev {
        // A review bundle must never take the real product's name,
        // directories, identifier, or URL scheme; macOS compares these
        // without case.
        let production =
            yard_build_config::read(&repository().join(PRODUCTION_CONFIG)).map_err(refuse)?;
        let file_names = (
            yard_product::file_name(&product.name),
            yard_product::file_name(&production.name),
        );
        let mut pairs = vec![
            ("name", &product.name, &production.name),
            ("identifier", &product.identifier, &production.identifier),
            ("url_scheme", &product.url_scheme, &production.url_scheme),
        ];
        if linux {
            // Two names that differ only outside their letters and digits
            // share their Linux files and directories.
            pairs.push(("name", &file_names.0, &file_names.1));
        }
        for (key, value, real) in pairs {
            if value.to_lowercase() == real.to_lowercase() {
                return Err(refuse(format!(
                    "{}: --dev requires a `{key}` different from the production product's",
                    config.display()
                )));
            }
        }
    }
    Ok(product)
}

fn is_path_component(value: &str) -> bool {
    !value.contains('/') && value != "." && value != ".."
}

/// The `KEY=VALUE` pairs of `--env`, each KEY shell-style.
fn env_pairs(env: &[String]) -> Result<Vec<(&str, &str)>, String> {
    env.iter()
        .map(|pair| {
            pair.split_once('=')
                .filter(|(key, _)| is_shell_name(key))
                .ok_or_else(|| format!("--env {pair:?} requires KEY=VALUE with a shell-style KEY"))
        })
        .collect()
}

fn is_shell_name(key: &str) -> bool {
    let mut characters = key.chars();
    characters
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && characters.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The selected binary's executable path from Cargo's JSON artifacts.
fn reported_executable(report: &[u8], binary: &str) -> Result<PathBuf, String> {
    String::from_utf8_lossy(report)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| {
            message["reason"] == "compiler-artifact"
                && message["target"]["name"] == binary
                && is_binary(&message["target"])
        })
        .filter_map(|message| message["executable"].as_str().map(PathBuf::from))
        .next_back()
        .ok_or_else(|| format!("cargo reported no {binary} executable"))
}

/// The staging folder `<out>/.bundle-<system>.<pid>-<n>`, inside `out` so what
/// is made there moves into place by a rename. It is closed on every path, a
/// panic too: dropped without [`Staging::close`], it closes as after a
/// failure.
struct Staging {
    path: PathBuf,
    /// What [`Staging::publish`] replaced in `out`, in order.
    replaced: Vec<Replaced>,
    closed: bool,
}

/// A file of `out` that a publish replaced, so a failure can undo it.
struct Replaced {
    target: PathBuf,
    /// Where the file that `target` held waits, in the staging folder.
    previous: Option<PathBuf>,
    /// Whether the new file is at `target`.
    moved: bool,
}

impl Staging {
    fn new(out: &Path, system: &str) -> io::Result<Self> {
        for attempt in 0.. {
            let path = out.join(format!(".bundle-{system}.{}-{attempt}", std::process::id()));
            match fs::create_dir(&path) {
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                result => {
                    return result.map(|()| Self {
                        path,
                        replaced: Vec::new(),
                        closed: false,
                    });
                }
            }
        }
        unreachable!("an unbounded range ends")
    }

    /// Moves `from` to `to`, keeping what `to` held in the staging folder
    /// until the folder closes.
    /// A file is never published over a directory, nor over a link to one:
    /// what `to` held would be moved into the staging folder, and removed
    /// with it.
    fn publish(&mut self, from: &Path, to: &Path) -> Result<(), String> {
        if from.symlink_metadata().is_ok_and(|from| from.is_file()) {
            replaceable_file(to)?;
        }
        let previous = if exists(to) {
            let previous = self.path.join(format!("previous-{}", self.replaced.len()));
            rename(to, &previous)?;
            Some(previous)
        } else {
            None
        };
        self.replaced.push(Replaced {
            target: to.to_owned(),
            previous,
            moved: false,
        });
        rename(from, to)?;
        if let Some(replaced) = self.replaced.last_mut() {
            replaced.moved = true;
        }
        Ok(())
    }

    /// Removes the folder. After a failure (`published` false), first takes
    /// back what was published, last first, and puts each previous file
    /// back; a previous file that cannot be put back keeps the folder, and
    /// is returned.
    fn close(&mut self, published: bool) -> Vec<PathBuf> {
        self.closed = true;
        let mut retained = Vec::new();
        if !published {
            let replaced = std::mem::take(&mut self.replaced);
            for (index, replaced) in replaced.into_iter().enumerate().rev() {
                let target = &replaced.target;
                if replaced.moved
                    && let Err(reason) =
                        rename(target, &self.path.join(format!("unpublished-{index}")))
                {
                    log::warn!("could not take back {}: {reason}", target.display());
                }
                if let Some(previous) = replaced.previous {
                    if !exists(target) && rename(&previous, target).is_ok() {
                        tracing::debug!(target = %target.display(), "previous file put back");
                    } else {
                        log::warn!(
                            "previous {} retained at {}",
                            target.display(),
                            previous.display()
                        );
                        retained.push(previous);
                    }
                }
            }
        }
        if retained.is_empty()
            && let Err(error) = fs::remove_dir_all(&self.path)
        {
            log::warn!("could not remove {}: {error}", self.path.display());
        }
        retained
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if !self.closed {
            self.close(false);
        }
    }
}

/// The steps that work in the staging folder: the icon, the bundle, the
/// signature, the notarization, the zip, and the disk image; then, unless
/// the run was interrupted, the app, the zip, and the disk image are
/// published together.
fn in_temp(
    staging: &mut Staging,
    job: &Job,
    session: &Session,
    runner: &mut dyn Runner,
) -> Result<Bundled, Failure> {
    let Job {
        product,
        options,
        binary,
        out,
        signing,
        interrupt,
    } = *job;
    let destination = &out.join(format!("{}.app", product.name));
    let temp = staging.path.clone();
    let temp = temp.as_path();
    let icns =
        make_icns(temp, &product.icon, runner).map_err(|reason| Failure::new("icon", reason))?;

    step("bundle", interrupt)?;
    let bundle = lay_out(product, options, binary, temp, Some(&icns)).map_err(|error| {
        Failure::new("bundle", format!("could not lay out the bundle: {error}"))
    })?;
    runner
        .run(
            Command::new("plutil")
                .arg("-lint")
                .arg(bundle.join("Contents/Info.plist")),
        )
        .map_err(|reason| Failure::new("bundle", reason))?;

    step("sign", interrupt)?;
    let sign = |reason| Failure::new("sign", reason);
    match signing.identity() {
        // The entitlements apply ad hoc too, as in Zed's local bundles.
        None => {
            let mut codesign = Command::new("codesign");
            codesign.args(["--force", "--deep"]);
            if let Some(file) = &product.macos.entitlements {
                codesign.arg("--entitlements").arg(file);
            }
            runner
                .run(codesign.args(["--sign", "-"]).arg(&bundle))
                .map_err(sign)?
        }
        Some(identity) => {
            // The binary, then the app, as Zed's `script/bundle-mac` does.
            let suffix = if options.dev { "-bin" } else { "" };
            let binary = bundle
                .join("Contents/MacOS")
                .join(format!("{}{suffix}", product.name));
            runner
                .run(&mut identity.codesign(&binary, true, None))
                .map_err(sign)?;
            runner
                .run(&mut identity.codesign(&bundle, true, None))
                .map_err(sign)?;
        }
    }
    runner
        .run(
            Command::new("codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(&bundle),
        )
        .map_err(sign)?;

    if let Some(identity) = signing.identity().filter(|identity| identity.notarizes()) {
        step("notarize", interrupt)?;
        let notarize = |reason| Failure::new("notarize", reason);
        let submitted = temp.join("notarize.zip");
        runner
            .run(
                Command::new("ditto")
                    .args(["-c", "-k", "--keepParent"])
                    .arg(&bundle)
                    .arg(&submitted),
            )
            .map_err(notarize)?;
        session
            .notarize(identity, &submitted, runner)
            .map_err(notarize)?;
        fs::remove_file(&submitted).map_err(|error| {
            notarize(format!("could not remove {}: {error}", submitted.display()))
        })?;
        step("staple", interrupt)?;
        staple(&bundle, runner).map_err(|reason| Failure::new("staple", reason))?;
    }

    let zip = if options.zip {
        step("zip", interrupt)?;
        let zip = |reason| Failure::new("zip", reason);
        let staged = temp.join("release.zip");
        runner
            .run(
                Command::new("ditto")
                    .args(["-c", "-k", "--keepParent"])
                    .arg(&bundle)
                    .arg(&staged),
            )
            .map_err(zip)?;
        // The digest comes from the staged zip, so no published zip lacks
        // one.
        let digest = sha256(&["shasum", "-a", "256"], &staged, runner).map_err(zip)?;
        Some((staged, digest))
    } else {
        None
    };
    let dmg = if product.macos.dmg {
        step("dmg", interrupt)?;
        let dmg = |reason| Failure::new("dmg", reason);
        let image = disk_image(temp, product, &bundle, runner).map_err(dmg)?;
        match signing.identity() {
            None => tracing::debug!("the app is signed ad hoc: the disk image is not signed"),
            Some(identity) => {
                runner
                    .run(&mut identity.codesign(
                        &image,
                        false,
                        Some(&format!("{}.dmg", product.identifier)),
                    ))
                    .map_err(dmg)?;
                if identity.notarizes() {
                    step("notarize", interrupt)?;
                    session
                        .notarize(identity, &image, runner)
                        .map_err(|reason| Failure::new("notarize", reason))?;
                    step("staple", interrupt)?;
                    staple(&image, runner).map_err(|reason| Failure::new("staple", reason))?;
                }
            }
        }
        let digest = sha256(&["shasum", "-a", "256"], &image, runner).map_err(dmg)?;
        Some((image, digest))
    } else {
        None
    };

    // Everything is published together, or nothing is: a failure takes back
    // what was moved and puts the previous files back.
    not_interrupted("publish", interrupt)?;
    tracing::debug!(destination = %destination.display(), "publish");
    staging
        .publish(&bundle, destination)
        .map_err(|reason| Failure::new("bundle", reason))?;
    let zip = match zip {
        Some((staged, digest)) => {
            let archive = out.join(format!("{}-{}.zip", product.name, product.version));
            staging
                .publish(&staged, &archive)
                .map_err(|reason| Failure::new("zip", reason))?;
            tracing::debug!(archive = %archive.display(), sha256 = digest, "zipped");
            Some((archive, digest))
        }
        None => None,
    };
    let dmg = match dmg {
        Some((image, digest)) => {
            let published = out.join(format!("{}-{}.dmg", product.name, product.version));
            staging
                .publish(&image, &published)
                .map_err(|reason| Failure::new("dmg", reason))?;
            tracing::debug!(dmg = %published.display(), sha256 = digest, "disk image");
            Some((published, digest))
        }
        None => None,
    };
    Ok(Bundled {
        app: destination.to_owned(),
        zip,
        dmg,
    })
}

/// Makes `<temp>/release.dmg` holding the app and a link to
/// `/Applications`, as Zed's `script/bundle-mac` does.
fn disk_image(
    temp: &Path,
    product: &BuildConfig,
    app: &Path,
    runner: &mut dyn Runner,
) -> Result<PathBuf, String> {
    let staging = temp.join("dmg");
    fs::create_dir(&staging)
        .map_err(|error| format!("could not create {}: {error}", staging.display()))?;
    runner.run(
        Command::new("ditto")
            .arg(app)
            .arg(staging.join(format!("{}.app", product.name))),
    )?;
    symlink("/Applications", &staging.join("Applications"))
        .map_err(|error| format!("could not link /Applications: {error}"))?;
    let image = temp.join("release.dmg");
    runner.run(
        Command::new("hdiutil")
            .args(["create", "-volname", &product.name, "-srcfolder"])
            .arg(&staging)
            .args(["-ov", "-format", "UDZO"])
            .arg(&image),
    )?;
    Ok(image)
}

#[cfg(unix)]
fn symlink(target: &str, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn symlink(_: &str, _: &Path) -> io::Result<()> {
    Err(io::Error::other("symbolic links need a Unix system"))
}

fn staple(path: &Path, runner: &mut dyn Runner) -> Result<(), String> {
    runner.run(Command::new("xcrun").args(["stapler", "staple"]).arg(path))
}

/// Makes `<temp>/product.icns` from the PNG, at 16 to 512 pixels, each at 1x
/// and 2x.
fn make_icns(temp: &Path, icon: &Path, runner: &mut dyn Runner) -> Result<PathBuf, String> {
    let iconset = temp.join("product.iconset");
    fs::create_dir(&iconset)
        .map_err(|error| format!("could not create {}: {error}", iconset.display()))?;
    for size in [16, 32, 128, 256, 512] {
        for (pixels, suffix) in [(size, ""), (size * 2, "@2x")] {
            let pixels = pixels.to_string();
            runner.output(
                Command::new("sips")
                    .args(["-z", &pixels, &pixels])
                    .arg(icon)
                    .arg("--out")
                    .arg(iconset.join(format!("icon_{size}x{size}{suffix}.png"))),
            )?;
        }
    }
    let icns = temp.join("product.icns");
    runner.run(
        Command::new("iconutil")
            .args(["-c", "icns"])
            .arg(&iconset)
            .arg("-o")
            .arg(&icns),
    )?;
    Ok(icns)
}

/// The SHA-256 of `file` in hex, from `command` (such as `shasum -a 256`)
/// run on it.
fn sha256(command: &[&str], file: &Path, runner: &mut dyn Runner) -> Result<String, String> {
    let output = runner.output(Command::new(command[0]).args(&command[1..]).arg(file))?;
    String::from_utf8_lossy(&output)
        .split_whitespace()
        .next()
        .filter(|digest| digest.len() == 64 && digest.chars().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_owned)
        .ok_or_else(|| format!("{} printed no SHA-256", command[0]))
}

/// Lays out `<folder>/<name>.app`: the binary (behind a wrapper running `dev`
/// with `--dev`), the icon when `icns` is given, and `Info.plist`.
pub fn lay_out(
    product: &BuildConfig,
    options: &MacosArgs,
    binary: &Path,
    folder: &Path,
    icns: Option<&Path>,
) -> io::Result<PathBuf> {
    let bundle = folder.join(format!("{}.app", product.name));
    let contents = bundle.join("Contents");
    fs::create_dir_all(contents.join("MacOS"))?;
    fs::create_dir_all(contents.join("Resources"))?;
    let executable = contents.join("MacOS").join(&product.name);
    if options.dev {
        fs::copy(
            binary,
            executable.with_file_name(format!("{}-bin", product.name)),
        )?;
        fs::write(&executable, WRAPPER)?;
        make_executable(&executable)?;
    } else {
        fs::copy(binary, &executable)?;
    }
    if let Some(icns) = icns {
        fs::rename(
            icns,
            contents
                .join("Resources")
                .join(format!("{}.icns", product.name)),
        )?;
    }
    let env = env_pairs(&options.env).map_err(io::Error::other)?;
    fs::write(contents.join("Info.plist"), info_plist(product, &env))?;
    Ok(bundle)
}

/// The executable of a review bundle: it runs the binary beside it with
/// `dev`.
pub const WRAPPER: &str = "#!/bin/sh\nexec \"$0-bin\" dev \"$@\"\n";

fn make_executable(path: &Path) -> io::Result<()> {
    set_mode(path, 0o755)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_: &Path, _: u32) -> io::Result<()> {
    Ok(())
}

/// The text of `Info.plist`, every value XML-escaped.
pub fn info_plist(product: &BuildConfig, env: &[(&str, &str)]) -> String {
    let name = xml_escape(&product.name);
    let version = xml_escape(&product.version);
    let identifier = xml_escape(&product.identifier);
    let scheme = xml_escape(&product.url_scheme);
    let mut text = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>{name}</string>
    <key>CFBundleDisplayName</key><string>{name}</string>
    <key>CFBundleIdentifier</key><string>{identifier}</string>
    <key>CFBundleShortVersionString</key><string>{version}</string>
    <key>CFBundleVersion</key><string>{version}</string>
    <key>CFBundleExecutable</key><string>{name}</string>
    <key>CFBundleIconFile</key><string>{name}.icns</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleURLTypes</key>
    <array>
        <dict>
            <key>CFBundleURLName</key><string>{identifier}</string>
            <key>CFBundleURLSchemes</key><array><string>{scheme}</string></array>
        </dict>
    </array>
    <key>LSMinimumSystemVersion</key><string>12.0</string>
    <key>NSHighResolutionCapable</key><true/>
"#
    );
    if !env.is_empty() {
        text.push_str("    <key>LSEnvironment</key>\n    <dict>\n");
        for (key, value) in env {
            text.push_str(&format!(
                "        <key>{}</key><string>{}</string>\n",
                xml_escape(key),
                xml_escape(value)
            ));
        }
        text.push_str("    </dict>\n");
    }
    text.push_str("</dict>\n</plist>\n");
    text
}

/// Escapes `&`, `<`, `>`, and a carriage return, which XML would otherwise
/// read as a line break.
pub fn xml_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\r' => escaped.push_str("&#13;"),
            c => escaped.push(c),
        }
    }
    escaped
}

/// Whether a file may be published at `path`: nothing is there, or a
/// regular file, which publishing keeps until it succeeds.
fn replaceable_file(path: &Path) -> Result<(), String> {
    if exists(path) && !path.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return Err(format!(
            "{} exists and is not a file; move it away first",
            path.display()
        ));
    }
    Ok(())
}

fn exists(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

fn rename(from: &Path, to: &Path) -> Result<(), String> {
    fs::rename(from, to).map_err(|error| {
        format!(
            "could not move {} to {}: {error}",
            from.display(),
            to.display()
        )
    })
}
