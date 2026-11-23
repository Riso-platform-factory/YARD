#![cfg(feature = "dev")]
//! `cargo xtask bundle` packages the workspace application given by
//! `--package` (defaulting to the one default member `yard`), taking
//! `<package folder>/product.toml` when `--config` is omitted, running the
//! `yard` stages check regardless of package, and refusing `--dev` on
//! packages other than `yard` or unknown packages at step `package` before
//! anything is tested or built.

#[allow(dead_code)]
mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use support::{
    FakeRunner, Plist, Recorded, assert_only_metadata, parse_plist, tempdir, under_test,
};
use xtask::{
    Captured, Interrupt, LinuxArgs, MacosArgs, Runner, Secrets, WindowsArgs, WindowsHost,
    WindowsSecrets, bundle_linux, bundle_macos, bundle_windows,
};

fn plist_of(app: &Path) -> Plist {
    parse_plist(&std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap())
}

fn windows_host(temp: &Path) -> WindowsHost {
    let inno = temp.join("inno");
    std::fs::create_dir_all(&inno).unwrap();
    std::fs::write(inno.join("ISCC.exe"), b"").unwrap();
    let root = temp.join("Windows");
    std::fs::create_dir_all(root.join("System32")).unwrap();
    for tool in ["tar.exe", "certutil.exe"] {
        std::fs::write(root.join("System32").join(tool), b"").unwrap();
    }
    WindowsHost {
        system: "windows".into(),
        path: Some(inno.into_os_string()),
        program_files_x86: None,
        system_root: Some(root.into_os_string()),
        xtask: None,
    }
}

fn product_config_env(command: &Recorded) -> &str {
    command
        .envs
        .iter()
        .find(|(key, _)| key == "PRODUCT_CONFIG")
        .and_then(|(_, value)| value.as_deref())
        .expect("PRODUCT_CONFIG set on cargo build")
}

/// Checks the first three commands of a successful release bundle: `cargo
/// metadata`, the `yard` stages check, and `cargo build` for `package`/`binary`
/// with `PRODUCT_CONFIG` pointing at `expected_config`.
fn assert_cargo_commands(runner: &FakeRunner, package: &str, binary: &str, expected_config: &Path) {
    assert!(
        runner.commands.len() >= 3,
        "expected metadata, stages, and build: {:?}",
        runner.commands
    );

    let metadata = &runner.commands[0];
    assert_eq!(metadata.program, "cargo");
    assert_eq!(metadata.dir.as_deref(), Some(xtask::repository().as_path()));
    assert_eq!(
        metadata.args,
        ["metadata", "--no-deps", "--format-version", "1"]
    );

    let stages = &runner.commands[1];
    assert_eq!(stages.program, "cargo");
    assert_eq!(stages.args, ["test", "-p", "yard", "--test", "stages"]);

    let build = &runner.commands[2];
    assert_eq!(build.program, "cargo");
    assert_eq!(
        build.args,
        [
            "build",
            "--release",
            "-p",
            package,
            "--bin",
            binary,
            "--no-default-features",
            "--message-format=json-render-diagnostics",
        ]
    );

    let config = Path::new(product_config_env(build));
    assert!(
        config.is_absolute(),
        "PRODUCT_CONFIG should be absolute: {config:?}"
    );
    assert_eq!(
        config.canonicalize().unwrap(),
        expected_config.canonicalize().unwrap()
    );
}

#[test]
fn without_package_bundles_default_member_yard_and_its_config() {
    let expected_config = xtask::repository().join("app/yard/product.toml");

    // macOS
    let temp = tempdir();
    let out = temp.path().join("macos-out");
    let mut runner = FakeRunner::new(temp.path());
    let bundled = bundle_macos(
        &MacosArgs {
            out: Some(out.clone()),
            ..MacosArgs::default()
        },
        &Secrets::default(),
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap();
    assert_cargo_commands(&runner, "yard", "yard", &expected_config);
    assert_eq!(bundled.app, out.join("yard.app"));
    let plist = plist_of(&bundled.app);
    assert_eq!(plist.get("CFBundleName"), Some(&Plist::string("yard")));
    assert_eq!(
        plist.get("CFBundleIdentifier"),
        Some(&Plist::string("vn.risotech.yard"))
    );

    // Linux
    let temp = tempdir();
    let out = temp.path().join("linux-out");
    let mut runner = FakeRunner::new(temp.path());
    let bundled = bundle_linux(
        &LinuxArgs {
            out: Some(out.clone()),
            ..LinuxArgs::default()
        },
        "linux",
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap();
    assert_cargo_commands(&runner, "yard", "yard", &expected_config);
    assert_eq!(bundled.app, out.join("yard.app"));

    // Windows
    let temp = tempdir();
    let out = temp.path().join("windows-out");
    let mut runner = FakeRunner::new(temp.path());
    let bundled = bundle_windows(
        &WindowsArgs {
            out: Some(out.clone()),
            ..WindowsArgs::default()
        },
        &windows_host(temp.path()),
        &WindowsSecrets::default(),
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap();
    assert_cargo_commands(&runner, "yard", "yard", &expected_config);
    assert_eq!(
        bundled.installer.0,
        out.join(format!("yard-0.1.0-{}-setup.exe", std::env::consts::ARCH))
    );
}

#[test]
fn package_north_star_without_config_uses_north_star_config_and_yard_stages() {
    let expected_config =
        xtask::repository().join("tooling/xtask/tests/configs/north_star/product.toml");

    // macOS
    let temp = tempdir();
    let out = temp.path().join("macos-out");
    let mut runner = FakeRunner::new(temp.path());
    let bundled = bundle_macos(
        &MacosArgs {
            package: Some("north_star".into()),
            out: Some(out.clone()),
            ..MacosArgs::default()
        },
        &Secrets::default(),
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap();
    assert_cargo_commands(&runner, "north_star", "north_star", &expected_config);
    assert_eq!(bundled.app, out.join("North Star.app"));
    let plist = plist_of(&bundled.app);
    assert_eq!(
        plist.get("CFBundleName"),
        Some(&Plist::string("North Star"))
    );
    assert_eq!(
        plist.get("CFBundleIdentifier"),
        Some(&Plist::string("test.yard.northstar"))
    );
    assert_eq!(
        plist.get("CFBundleExecutable"),
        Some(&Plist::string("North Star"))
    );
    assert_eq!(
        std::fs::read(bundled.app.join("Contents/MacOS/North Star")).unwrap(),
        b"the yard binary"
    );

    // Linux
    let temp = tempdir();
    let out = temp.path().join("linux-out");
    let mut runner = FakeRunner::new(temp.path());
    let bundled = bundle_linux(
        &LinuxArgs {
            package: Some("north_star".into()),
            out: Some(out.clone()),
            ..LinuxArgs::default()
        },
        "linux",
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap();
    assert_cargo_commands(&runner, "north_star", "north_star", &expected_config);
    assert_eq!(bundled.app, out.join("north-star.app"));
    assert_eq!(
        bundled.archive.0,
        out.join(format!(
            "north-star-0.1.0-linux-{}.tar.gz",
            std::env::consts::ARCH
        ))
    );
    assert_eq!(
        std::fs::read(bundled.app.join("bin/north-star")).unwrap(),
        b"the yard binary"
    );

    // Windows
    let temp = tempdir();
    let out = temp.path().join("windows-out");
    let mut runner = FakeRunner::new(temp.path());
    let bundled = bundle_windows(
        &WindowsArgs {
            package: Some("north_star".into()),
            out: Some(out.clone()),
            ..WindowsArgs::default()
        },
        &windows_host(temp.path()),
        &WindowsSecrets::default(),
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap();
    assert_cargo_commands(&runner, "north_star", "north_star", &expected_config);
    assert_eq!(
        bundled.installer.0,
        out.join(format!(
            "North Star-0.1.0-{}-setup.exe",
            std::env::consts::ARCH
        ))
    );
    assert_eq!(
        bundled.zip.0,
        out.join(format!(
            "North Star-0.1.0-windows-{}.zip",
            std::env::consts::ARCH
        ))
    );
}

#[test]
fn package_north_star_with_explicit_config_overrides_default_config() {
    let config = under_test("plain");

    let temp = tempdir();
    let out = temp.path().join("macos-out");
    let mut runner = FakeRunner::new(temp.path());
    let bundled = bundle_macos(
        &MacosArgs {
            package: Some("north_star".into()),
            config: Some(config.clone()),
            out: Some(out.clone()),
            ..MacosArgs::default()
        },
        &Secrets::default(),
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap();
    assert_cargo_commands(&runner, "north_star", "north_star", &config);
    assert_eq!(bundled.app, out.join("Lab.app"));

    let temp = tempdir();
    let out = temp.path().join("linux-out");
    let mut runner = FakeRunner::new(temp.path());
    let bundled = bundle_linux(
        &LinuxArgs {
            package: Some("north_star".into()),
            config: Some(config.clone()),
            out: Some(out.clone()),
            ..LinuxArgs::default()
        },
        "linux",
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap();
    assert_cargo_commands(&runner, "north_star", "north_star", &config);
    assert_eq!(bundled.app, out.join("lab.app"));

    let temp = tempdir();
    let out = temp.path().join("windows-out");
    let mut runner = FakeRunner::new(temp.path());
    let bundled = bundle_windows(
        &WindowsArgs {
            package: Some("north_star".into()),
            config: Some(config.clone()),
            out: Some(out.clone()),
            ..WindowsArgs::default()
        },
        &windows_host(temp.path()),
        &WindowsSecrets::default(),
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap();
    assert_cargo_commands(&runner, "north_star", "north_star", &config);
    assert_eq!(
        bundled.installer.0,
        out.join(format!("Lab-0.1.0-{}-setup.exe", std::env::consts::ARCH))
    );
}

#[test]
fn package_north_star_with_dev_is_refused_at_package_before_stages_or_build() {
    // macOS, both without and with an explicit config.
    for config in [None, Some(under_test("plain"))] {
        let temp = tempdir();
        let out = temp.path().join("macos-out");
        let mut runner = FakeRunner::new(temp.path());
        let failure = bundle_macos(
            &MacosArgs {
                package: Some("north_star".into()),
                config,
                out: Some(out.clone()),
                dev: true,
                ..MacosArgs::default()
            },
            &Secrets::default(),
            &Interrupt::default(),
            &mut runner,
        )
        .unwrap_err();
        assert_eq!(failure.step, "package");
        assert!(failure.is_refusal(), "{failure}");
        assert_only_metadata(&runner);
        assert!(!out.exists());
    }

    // Linux
    let temp = tempdir();
    let out = temp.path().join("linux-out");
    let mut runner = FakeRunner::new(temp.path());
    let failure = bundle_linux(
        &LinuxArgs {
            package: Some("north_star".into()),
            config: Some(under_test("plain")),
            out: Some(out.clone()),
            dev: true,
            ..LinuxArgs::default()
        },
        "linux",
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "package");
    assert!(failure.is_refusal(), "{failure}");
    assert_only_metadata(&runner);
    assert!(!out.exists());

    // Windows
    let temp = tempdir();
    let out = temp.path().join("windows-out");
    let mut runner = FakeRunner::new(temp.path());
    let failure = bundle_windows(
        &WindowsArgs {
            package: Some("north_star".into()),
            config: Some(under_test("plain")),
            out: Some(out.clone()),
            dev: true,
            ..WindowsArgs::default()
        },
        &windows_host(temp.path()),
        &WindowsSecrets::default(),
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "package");
    assert!(failure.is_refusal(), "{failure}");
    assert_only_metadata(&runner);
    assert!(!out.exists());
}

#[test]
fn unknown_package_is_refused_at_package_before_stages_or_build() {
    // macOS
    let temp = tempdir();
    let out = temp.path().join("macos-out");
    let mut runner = FakeRunner::new(temp.path());
    let failure = bundle_macos(
        &MacosArgs {
            package: Some("no_such_package".into()),
            out: Some(out.clone()),
            ..MacosArgs::default()
        },
        &Secrets::default(),
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "package");
    assert!(failure.is_refusal(), "{failure}");
    assert!(failure.reason.contains("no_such_package"), "{failure}");
    assert_only_metadata(&runner);
    assert!(!out.exists());

    // Linux
    let temp = tempdir();
    let out = temp.path().join("linux-out");
    let mut runner = FakeRunner::new(temp.path());
    let failure = bundle_linux(
        &LinuxArgs {
            package: Some("no_such_package".into()),
            out: Some(out.clone()),
            ..LinuxArgs::default()
        },
        "linux",
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "package");
    assert!(failure.is_refusal(), "{failure}");
    assert!(failure.reason.contains("no_such_package"), "{failure}");
    assert_only_metadata(&runner);
    assert!(!out.exists());

    // Windows
    let temp = tempdir();
    let out = temp.path().join("windows-out");
    let mut runner = FakeRunner::new(temp.path());
    let failure = bundle_windows(
        &WindowsArgs {
            package: Some("no_such_package".into()),
            out: Some(out.clone()),
            ..WindowsArgs::default()
        },
        &windows_host(temp.path()),
        &WindowsSecrets::default(),
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "package");
    assert!(failure.is_refusal(), "{failure}");
    assert!(failure.reason.contains("no_such_package"), "{failure}");
    assert_only_metadata(&runner);
    assert!(!out.exists());
}

/// Delegates to `FakeRunner`, except `cargo build` emits only non-matching
/// artifacts (`yard` bin and `north_star` lib) to verify executable selection.
struct WrongArtifactRunner {
    inner: FakeRunner,
    binary: PathBuf,
}

impl Runner for WrongArtifactRunner {
    fn run(&mut self, command: &mut Command) -> Result<(), String> {
        self.inner.run(command)
    }

    fn output(&mut self, command: &mut Command) -> Result<Vec<u8>, String> {
        let is_build = command.get_args().next().is_some_and(|arg| arg == "build");
        let output = self.inner.output(command)?;
        if is_build {
            return Ok(format!(
                "{}\n{}\n{}\n",
                serde_json::json!({
                    "reason": "compiler-artifact",
                    "target": {"name": "yard", "kind": ["bin"]},
                    "executable": self.binary,
                }),
                serde_json::json!({
                    "reason": "compiler-artifact",
                    "target": {"name": "north_star", "kind": ["lib"]},
                    "executable": self.binary,
                }),
                serde_json::json!({"reason": "build-finished", "success": true}),
            )
            .into_bytes());
        }
        Ok(output)
    }

    fn report(&mut self, command: &mut Command) -> Result<(bool, Vec<u8>), String> {
        self.inner.report(command)
    }

    fn input(&mut self, command: &mut Command, input: &[u8]) -> Result<Vec<u8>, String> {
        self.inner.input(command, input)
    }

    fn capture(&mut self, command: &mut Command) -> Result<Captured, String> {
        self.inner.capture(command)
    }
}

#[test]
fn build_fails_when_cargo_reports_no_matching_binary_executable() {
    let temp = tempdir();
    let inner = FakeRunner::new(temp.path());
    let binary = inner.binary.clone();
    let mut runner = WrongArtifactRunner { inner, binary };
    let failure = bundle_macos(
        &MacosArgs {
            package: Some("north_star".into()),
            out: Some(temp.path().join("out")),
            ..MacosArgs::default()
        },
        &Secrets::default(),
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "build");
    assert!(!failure.is_refusal());
    assert_eq!(failure.reason, "cargo reported no north_star executable");
}

#[test]
fn failing_cargo_metadata_is_a_metadata_failure_not_a_refusal() {
    // macOS
    let temp = tempdir();
    let out = temp.path().join("macos-out");
    let mut runner = FakeRunner::new(temp.path());
    runner.fail = Some("cargo metadata");
    let failure = bundle_macos(
        &MacosArgs {
            package: Some("north_star".into()),
            out: Some(out.clone()),
            ..MacosArgs::default()
        },
        &Secrets::default(),
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "metadata");
    assert!(!failure.is_refusal(), "{failure}");
    assert_only_metadata(&runner);
    assert!(!out.exists());

    // Linux
    let temp = tempdir();
    let out = temp.path().join("linux-out");
    let mut runner = FakeRunner::new(temp.path());
    runner.fail = Some("cargo metadata");
    let failure = bundle_linux(
        &LinuxArgs {
            package: Some("north_star".into()),
            out: Some(out.clone()),
            ..LinuxArgs::default()
        },
        "linux",
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "metadata");
    assert!(!failure.is_refusal(), "{failure}");
    assert_only_metadata(&runner);
    assert!(!out.exists());

    // Windows
    let temp = tempdir();
    let out = temp.path().join("windows-out");
    let mut runner = FakeRunner::new(temp.path());
    runner.fail = Some("cargo metadata");
    let failure = bundle_windows(
        &WindowsArgs {
            package: Some("north_star".into()),
            out: Some(out.clone()),
            ..WindowsArgs::default()
        },
        &windows_host(temp.path()),
        &WindowsSecrets::default(),
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "metadata");
    assert!(!failure.is_refusal(), "{failure}");
    assert_only_metadata(&runner);
    assert!(!out.exists());
}

/// Delegates to `FakeRunner`, except `cargo metadata` prints non-JSON output.
struct InvalidMetadataRunner {
    inner: FakeRunner,
}

impl Runner for InvalidMetadataRunner {
    fn run(&mut self, command: &mut Command) -> Result<(), String> {
        self.inner.run(command)
    }

    fn output(&mut self, command: &mut Command) -> Result<Vec<u8>, String> {
        let is_metadata = command
            .get_args()
            .next()
            .is_some_and(|arg| arg == "metadata");
        let output = self.inner.output(command)?;
        if is_metadata {
            return Ok(b"not json".to_vec());
        }
        Ok(output)
    }

    fn report(&mut self, command: &mut Command) -> Result<(bool, Vec<u8>), String> {
        self.inner.report(command)
    }

    fn input(&mut self, command: &mut Command, input: &[u8]) -> Result<Vec<u8>, String> {
        self.inner.input(command, input)
    }

    fn capture(&mut self, command: &mut Command) -> Result<Captured, String> {
        self.inner.capture(command)
    }
}

#[test]
fn non_json_cargo_metadata_is_a_metadata_failure_not_a_refusal() {
    // macOS
    let temp = tempdir();
    let out = temp.path().join("macos-out");
    let mut runner = InvalidMetadataRunner {
        inner: FakeRunner::new(temp.path()),
    };
    let failure = bundle_macos(
        &MacosArgs {
            package: Some("north_star".into()),
            out: Some(out.clone()),
            ..MacosArgs::default()
        },
        &Secrets::default(),
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "metadata");
    assert!(!failure.is_refusal(), "{failure}");
    assert_only_metadata(&runner.inner);
    assert!(!out.exists());

    // Linux
    let temp = tempdir();
    let out = temp.path().join("linux-out");
    let mut runner = InvalidMetadataRunner {
        inner: FakeRunner::new(temp.path()),
    };
    let failure = bundle_linux(
        &LinuxArgs {
            package: Some("north_star".into()),
            out: Some(out.clone()),
            ..LinuxArgs::default()
        },
        "linux",
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "metadata");
    assert!(!failure.is_refusal(), "{failure}");
    assert_only_metadata(&runner.inner);
    assert!(!out.exists());

    // Windows
    let temp = tempdir();
    let out = temp.path().join("windows-out");
    let mut runner = InvalidMetadataRunner {
        inner: FakeRunner::new(temp.path()),
    };
    let failure = bundle_windows(
        &WindowsArgs {
            package: Some("north_star".into()),
            out: Some(out.clone()),
            ..WindowsArgs::default()
        },
        &windows_host(temp.path()),
        &WindowsSecrets::default(),
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "metadata");
    assert!(!failure.is_refusal(), "{failure}");
    assert_only_metadata(&runner.inner);
    assert!(!out.exists());
}

#[test]
fn interrupt_before_package_runs_no_commands() {
    // macOS
    let temp = tempdir();
    let out = temp.path().join("macos-out");
    let interrupt = Interrupt::default();
    interrupt.set();
    let mut runner = FakeRunner::new(temp.path());
    let failure = bundle_macos(
        &MacosArgs {
            package: Some("north_star".into()),
            out: Some(out.clone()),
            ..MacosArgs::default()
        },
        &Secrets::default(),
        &interrupt,
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(
        (failure.step, failure.reason.as_str()),
        ("package", "interrupted")
    );
    assert!(runner.commands.is_empty(), "{:?}", runner.commands);
    assert!(!out.exists());

    // Linux
    let temp = tempdir();
    let out = temp.path().join("linux-out");
    let interrupt = Interrupt::default();
    interrupt.set();
    let mut runner = FakeRunner::new(temp.path());
    let failure = bundle_linux(
        &LinuxArgs {
            package: Some("north_star".into()),
            out: Some(out.clone()),
            ..LinuxArgs::default()
        },
        "linux",
        &interrupt,
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(
        (failure.step, failure.reason.as_str()),
        ("package", "interrupted")
    );
    assert!(runner.commands.is_empty(), "{:?}", runner.commands);
    assert!(!out.exists());

    // Windows
    let temp = tempdir();
    let out = temp.path().join("windows-out");
    let interrupt = Interrupt::default();
    interrupt.set();
    let mut runner = FakeRunner::new(temp.path());
    let failure = bundle_windows(
        &WindowsArgs {
            package: Some("north_star".into()),
            out: Some(out.clone()),
            ..WindowsArgs::default()
        },
        &windows_host(temp.path()),
        &WindowsSecrets::default(),
        &interrupt,
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(
        (failure.step, failure.reason.as_str()),
        ("package", "interrupted")
    );
    assert!(runner.commands.is_empty(), "{:?}", runner.commands);
    assert!(!out.exists());
}
