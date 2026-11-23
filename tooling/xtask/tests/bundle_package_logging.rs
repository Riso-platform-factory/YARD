#![cfg(feature = "dev")]
//! When `--config` is omitted, each `bundle-*` group opens without a `config`
//! field; once the `package` step resolves, it logs a `DEBUG` event `package`
//! with `package`, `binary`, and `config` inside the group. A refusal at step
//! `package` logs `refused:` at `INFO` with `step=package` and never an
//! `ERROR`; a `cargo metadata` failure logs an `ERROR` at step `metadata`;
//! and an interrupt before the run logs `stopped: interrupted` at `INFO`
//! rather than `refused:`.
//!
//! This test captures `tracing` output and starts `yard_log`, which installs
//! a process-wide `log` logger, so it is alone in its file and no other test
//! in this binary reaches the same log lines.

#[allow(dead_code)]
mod support;

use std::path::Path;
use std::sync::{Arc, Mutex};
use support::{FakeRunner, tempdir};
use tracing::Level;
use xtask::{
    Interrupt, LinuxArgs, MacosArgs, Secrets, WindowsArgs, WindowsHost, WindowsSecrets,
    bundle_linux, bundle_macos, bundle_windows,
};

/// A group that opened, or a line logged inside one.
#[derive(Debug)]
enum Record {
    Group {
        id: tracing::span::Id,
        name: &'static str,
        fields: Vec<(String, String)>,
    },
    Line {
        group: Option<tracing::span::Id>,
        level: tracing::Level,
        message: String,
        fields: Vec<(String, String)>,
    },
}

#[derive(Debug, Clone)]
struct Line {
    level: Level,
    message: String,
    fields: Vec<(String, String)>,
}

/// The fields of a span or an event as `name=value` pairs. A `?`-formatted
/// string's quotes are stripped so `name`, `%name`, and `?name` all read as
/// the bare value.
#[derive(Default)]
struct Fields(Vec<(String, String)>);
impl tracing::field::Visit for Fields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.push((field.name().into(), value.into()));
    }
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let debug = format!("{value:?}");
        let value = debug
            .strip_prefix('"')
            .and_then(|inner| inner.strip_suffix('"'))
            .unwrap_or(debug.as_str());
        self.0.push((field.name().into(), value.into()));
    }
}

/// The groups that opened (with only their opening fields, matching
/// `yard_log`'s lifecycle trace) and the lines logged, in order; each line
/// keeps the group it was logged in, `None` when no group is open.
#[derive(Clone, Default)]
struct Trace {
    records: Arc<Mutex<Vec<Record>>>,
    open: Arc<Mutex<Vec<tracing::span::Id>>>,
}
impl tracing::Subscriber for Trace {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        let mut records = self.records.lock().unwrap();
        let id = tracing::span::Id::from_u64(records.len() as u64 + 1);
        records.push(Record::Group {
            id: id.clone(),
            name: attributes.metadata().name(),
            fields: fields.0,
        });
        id
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let message = fields
            .0
            .iter()
            .find(|(name, _)| name.as_str() == "message")
            .map(|(_, value)| value.clone())
            .unwrap_or_default();
        let group = self.open.lock().unwrap().last().cloned();
        self.records.lock().unwrap().push(Record::Line {
            group,
            level: *event.metadata().level(),
            message,
            fields: fields.0,
        });
    }
    fn enter(&self, id: &tracing::span::Id) {
        self.open.lock().unwrap().push(id.clone());
    }
    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(self.open.lock().unwrap().pop().as_ref(), Some(id));
    }
}

/// Starts `yard_log` as `xtask::main` does, with the lifecycle trace's own
/// output off, so the diagnostic lines (`log`) reach the test's subscriber as
/// they reach the lifecycle trace.
fn start_logs() {
    // SAFETY: the only test of this binary sets it before any thread reads
    // the environment.
    unsafe { std::env::set_var("XTASK_TEST_LIFECYCLE", "off") };
    yard_log::start(yard_product::Product::new(
        "xtask-test",
        "0.1.0",
        "dev.yard.xtask-test",
    ));
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

fn field_of<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn assert_package_event(lines: &[Line], package: &str, binary: &str, config: &str) {
    let events: Vec<&Line> = lines
        .iter()
        .filter(|line| line.level == Level::DEBUG && line.message == "package")
        .collect();
    assert_eq!(
        events.len(),
        1,
        "expected one DEBUG `package` event: {lines:?}"
    );
    let event = events[0];
    assert_eq!(
        field_of(&event.fields, "package"),
        Some(package),
        "package field on `package` event: {:?}",
        event.fields
    );
    assert_eq!(
        field_of(&event.fields, "binary"),
        Some(binary),
        "binary field on `package` event: {:?}",
        event.fields
    );
    assert_eq!(
        field_of(&event.fields, "config"),
        Some(config),
        "config field on `package` event: {:?}",
        event.fields
    );
}

#[test]
fn package_resolution_logs_package_event_refusals_failures_and_interrupts() {
    start_logs();
    let temp = tempdir();
    let expected_config = std::path::absolute(
        xtask::repository()
            .join("examples")
            .join("north_star")
            .join("product.toml"),
    )
    .unwrap();
    assert!(expected_config.is_absolute());
    let expected_config = expected_config.display().to_string();

    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        // 0: macOS ok without `--config`
        bundle_macos(
            &MacosArgs {
                package: Some("north_star".into()),
                out: Some(temp.path().join("macos-out")),
                ..MacosArgs::default()
            },
            &Secrets::default(),
            &Interrupt::default(),
            &mut FakeRunner::new(temp.path()),
        )
        .unwrap();

        // 1: macOS refused `--package north_star --dev`
        bundle_macos(
            &MacosArgs {
                package: Some("north_star".into()),
                out: Some(temp.path().join("macos-dev-out")),
                dev: true,
                ..MacosArgs::default()
            },
            &Secrets::default(),
            &Interrupt::default(),
            &mut FakeRunner::new(temp.path()),
        )
        .unwrap_err();

        // 2: macOS failed `cargo metadata`
        let mut failing_metadata = FakeRunner::new(temp.path());
        failing_metadata.fail = Some("cargo metadata");
        bundle_macos(
            &MacosArgs {
                package: Some("north_star".into()),
                out: Some(temp.path().join("macos-meta-fail-out")),
                ..MacosArgs::default()
            },
            &Secrets::default(),
            &Interrupt::default(),
            &mut failing_metadata,
        )
        .unwrap_err();

        // 3: macOS interrupted before the run starts
        let interrupt = Interrupt::default();
        interrupt.set();
        bundle_macos(
            &MacosArgs {
                package: Some("north_star".into()),
                out: Some(temp.path().join("macos-interrupted-out")),
                ..MacosArgs::default()
            },
            &Secrets::default(),
            &interrupt,
            &mut FakeRunner::new(temp.path()),
        )
        .unwrap_err();

        // Linux ok without `--config`
        bundle_linux(
            &LinuxArgs {
                package: Some("north_star".into()),
                out: Some(temp.path().join("linux-out")),
                ..LinuxArgs::default()
            },
            "linux",
            &Interrupt::default(),
            &mut FakeRunner::new(temp.path()),
        )
        .unwrap();

        // Windows ok without `--config`
        bundle_windows(
            &WindowsArgs {
                package: Some("north_star".into()),
                out: Some(temp.path().join("windows-out")),
                ..WindowsArgs::default()
            },
            &windows_host(temp.path()),
            &WindowsSecrets::default(),
            &Interrupt::default(),
            &mut FakeRunner::new(temp.path()),
        )
        .unwrap();
    });

    let records = trace.records.lock().unwrap();
    let groups_named = |target: &str| -> Vec<(tracing::span::Id, Vec<(String, String)>)> {
        records
            .iter()
            .filter_map(|record| match record {
                Record::Group { id, name, fields } if *name == target => {
                    Some((id.clone(), fields.clone()))
                }
                _ => None,
            })
            .collect()
    };
    let lines_in = |id: &tracing::span::Id| -> Vec<Line> {
        records
            .iter()
            .filter_map(|record| match record {
                Record::Line {
                    group,
                    level,
                    message,
                    fields,
                } if group.as_ref() == Some(id) => Some(Line {
                    level: *level,
                    message: message.clone(),
                    fields: fields.clone(),
                }),
                _ => None,
            })
            .collect()
    };

    let macos_groups = groups_named("bundle-macos");
    assert_eq!(macos_groups.len(), 4, "four macOS runs: {macos_groups:?}");

    // 0: macOS ok without `--config`
    assert_eq!(
        field_of(&macos_groups[0].1, "config"),
        None,
        "bundle-macos opens without a config field when --config is absent: {:?}",
        macos_groups[0].1
    );
    let macos_ok = lines_in(&macos_groups[0].0);
    assert_package_event(&macos_ok, "north_star", "north_star", &expected_config);
    let macos_steps: Vec<&str> = macos_ok
        .iter()
        .filter(|line| line.level == Level::DEBUG)
        .map(|line| line.message.as_str())
        .collect();
    for step in ["arguments", "package", "config", "stages", "build"] {
        assert!(
            macos_steps.contains(&step),
            "a debug line for {step}: {macos_ok:?}"
        );
    }
    assert!(
        macos_ok.iter().all(|line| line.level == Level::DEBUG),
        "a run that succeeds logs only debug steps: {macos_ok:?}"
    );

    // 1: macOS refused `--package north_star --dev`
    assert_eq!(
        field_of(&macos_groups[1].1, "config"),
        None,
        "refused bundle-macos opens without a config field: {:?}",
        macos_groups[1].1
    );
    let refusal = lines_in(&macos_groups[1].0);
    assert!(
        refusal.iter().any(|line| line.level == Level::INFO
            && line.message.starts_with("refused: ")
            && line.message.contains("--dev")
            && field_of(&line.fields, "step") == Some("package")),
        "a package refusal is an INFO `refused:` line with step=package mentioning --dev: {refusal:?}"
    );
    assert!(
        !refusal.iter().any(|line| line.level == Level::ERROR),
        "a package refusal is never an ERROR: {refusal:?}"
    );

    // 2: macOS failed `cargo metadata`
    let metadata_failure = lines_in(&macos_groups[2].0);
    assert!(
        metadata_failure
            .iter()
            .any(|line| line.level == Level::ERROR
                && line.message.starts_with("failed at metadata: ")),
        "a metadata failure is logged as an ERROR at step metadata: {metadata_failure:?}"
    );
    assert!(
        !metadata_failure
            .iter()
            .any(|line| line.message.starts_with("refused: ")),
        "a metadata failure is not logged as a refusal: {metadata_failure:?}"
    );

    // 3: macOS interrupted before the run starts
    let interrupted = lines_in(&macos_groups[3].0);
    assert!(
        interrupted.iter().any(|line| line.level == Level::INFO
            && line.message == "stopped: interrupted"
            && field_of(&line.fields, "step") == Some("package")),
        "an interrupt before the run logs `stopped: interrupted` at INFO with step=package: {interrupted:?}"
    );
    assert!(
        !interrupted
            .iter()
            .any(|line| line.message.starts_with("refused: ")),
        "an interrupt at step package is not logged as a refusal: {interrupted:?}"
    );
    assert!(
        !interrupted.iter().any(|line| line.level == Level::ERROR),
        "an interrupt is never an ERROR: {interrupted:?}"
    );

    // Linux ok without `--config`
    let linux_groups = groups_named("bundle-linux");
    assert_eq!(linux_groups.len(), 1, "one Linux run: {linux_groups:?}");
    assert_eq!(
        field_of(&linux_groups[0].1, "config"),
        None,
        "bundle-linux opens without a config field when --config is absent: {:?}",
        linux_groups[0].1
    );
    assert_package_event(
        &lines_in(&linux_groups[0].0),
        "north_star",
        "north_star",
        &expected_config,
    );

    // Windows ok without `--config`
    let windows_groups = groups_named("bundle-windows");
    assert_eq!(
        windows_groups.len(),
        1,
        "one Windows run: {windows_groups:?}"
    );
    assert_eq!(
        field_of(&windows_groups[0].1, "config"),
        None,
        "bundle-windows opens without a config field when --config is absent: {:?}",
        windows_groups[0].1
    );
    assert_package_event(
        &lines_in(&windows_groups[0].0),
        "north_star",
        "north_star",
        &expected_config,
    );
}
