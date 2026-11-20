#![cfg(all(feature = "dev", target_os = "macos"))]
//! `bundle_macos` runs inside the group `bundle-macos{config, dev, zip}`: a
//! `debug` line per step, a refusal at `info` and never an `ERROR`, and a
//! failed step at `error`.
//!
//! This test captures `tracing` output and starts `yard_log`, which installs
//! a process-wide `log` logger, so it is alone in its file and no other test
//! in this binary reaches the same log lines.

#[allow(dead_code)]
mod support;

use std::sync::{Arc, Mutex};
use support::{FakeRunner, base64, signed_config, tempdir, under_test};
use tracing::Level;
use xtask::{MacosArgs, bundle_macos};

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
    },
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

/// The groups that opened and the lines logged, in order; each line keeps the
/// group it was logged in, `None` when no group is open.
#[derive(Clone, Default)]
struct Trace {
    records: Arc<Mutex<Vec<Record>>>,
    open: Arc<Mutex<Vec<tracing::span::Id>>>,
    /// Every field of every group and line, to look for secrets.
    text: Arc<Mutex<String>>,
}
impl tracing::Subscriber for Trace {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        for (key, value) in &fields.0 {
            self.text
                .lock()
                .unwrap()
                .push_str(&format!("{key}={value}\n"));
        }
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
        for (key, value) in &fields.0 {
            self.text
                .lock()
                .unwrap()
                .push_str(&format!("{key}={value}\n"));
        }
        let message = fields
            .0
            .into_iter()
            .find(|(name, _)| name.as_str() == "message")
            .map(|(_, value)| value)
            .unwrap_or_default();
        let group = self.open.lock().unwrap().last().cloned();
        self.records.lock().unwrap().push(Record::Line {
            group,
            level: *event.metadata().level(),
            message,
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

#[test]
fn bundling_runs_inside_the_bundle_macos_group() {
    start_logs();
    let temp = tempdir();
    let config = under_test("plain");
    let out = temp.path().join("out");
    let options = MacosArgs {
        config: Some(config.clone()),
        out: Some(out),
        ..MacosArgs::default()
    };
    let refused = MacosArgs {
        env: vec!["YARD_HOME=/tmp".into()],
        zip: true,
        ..options.clone()
    };
    let signed_folder = temp.path().join("signed");
    std::fs::create_dir(&signed_folder).unwrap();
    let signed_config = signed_config(&signed_folder, true, true);
    let signed = MacosArgs {
        config: Some(signed_config.clone()),
        out: Some(temp.path().join("signed-out")),
        ..MacosArgs::default()
    };
    let secrets = xtask::Secrets {
        certificate: Some(base64(b"certificate hunter2")),
        certificate_password: Some("password hunter2".into()),
        notarization_key: Some(base64(b"key hunter2")),
        notarization_key_id: Some("KEYID".into()),
        notarization_issuer_id: Some("ISSUER".into()),
    };
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        bundle_macos(
            &options,
            &xtask::Secrets::default(),
            &xtask::Interrupt::default(),
            &mut FakeRunner::new(temp.path()),
        )
        .unwrap();
        bundle_macos(
            &refused,
            &xtask::Secrets::default(),
            &xtask::Interrupt::default(),
            &mut FakeRunner::new(temp.path()),
        )
        .unwrap_err();
        let mut failing = FakeRunner::new(temp.path());
        failing.fail = Some("codesign");
        bundle_macos(
            &options,
            &xtask::Secrets::default(),
            &xtask::Interrupt::default(),
            &mut failing,
        )
        .unwrap_err();
        bundle_macos(
            &signed,
            &secrets,
            &xtask::Interrupt::default(),
            &mut FakeRunner::new(temp.path()),
        )
        .unwrap();
        let mut stuck = FakeRunner::new(temp.path());
        stuck.fail = Some("security delete-keychain");
        bundle_macos(&signed, &secrets, &xtask::Interrupt::default(), &mut stuck).unwrap_err();
    });
    let records = trace.records.lock().unwrap();

    let groups: Vec<(tracing::span::Id, Vec<(String, String)>)> = records
        .iter()
        .filter_map(|record| match record {
            Record::Group { id, name, fields } if *name == "bundle-macos" => {
                Some((id.clone(), fields.clone()))
            }
            _ => None,
        })
        .collect();
    let field = |key: &str, value: &str| (key.to_owned(), value.to_owned());
    let config = config.display().to_string();
    let signed_config = signed_config.display().to_string();
    assert_eq!(
        groups
            .iter()
            .map(|(_, fields)| fields.clone())
            .collect::<Vec<_>>(),
        [
            vec![
                field("config", &config),
                field("dev", "false"),
                field("zip", "false")
            ],
            vec![
                field("config", &config),
                field("dev", "false"),
                field("zip", "true")
            ],
            vec![
                field("config", &config),
                field("dev", "false"),
                field("zip", "false")
            ],
            vec![
                field("config", &signed_config),
                field("dev", "false"),
                field("zip", "false")
            ],
            vec![
                field("config", &signed_config),
                field("dev", "false"),
                field("zip", "false")
            ],
        ],
        "one group per run, with its fields"
    );
    let lines_in = |id: &tracing::span::Id| -> Vec<(Level, String)> {
        records
            .iter()
            .filter_map(|record| match record {
                Record::Line {
                    group,
                    level,
                    message,
                } if group.as_ref() == Some(id) => Some((*level, message.clone())),
                _ => None,
            })
            .collect()
    };

    let bundled = lines_in(&groups[0].0);
    let debug_steps: Vec<&str> = bundled
        .iter()
        .filter(|(level, _)| *level == Level::DEBUG)
        .map(|(_, message)| message.as_str())
        .collect();
    for step in [
        "arguments",
        "config",
        "stages",
        "build",
        "icon",
        "bundle",
        "sign",
    ] {
        assert!(
            debug_steps.contains(&step),
            "a debug line for {step}: {bundled:?}"
        );
    }
    assert!(
        bundled.iter().all(|(level, _)| *level == Level::DEBUG),
        "a run that succeeds logs only steps: {bundled:?}"
    );

    let refusal = lines_in(&groups[1].0);
    assert!(
        refusal
            .iter()
            .any(|(level, message)| *level == Level::INFO
                && message == "refused: --env requires --dev"),
        "a refusal is an info line with the reason: {refusal:?}"
    );
    assert!(
        !refusal.iter().any(|(level, _)| *level == Level::ERROR),
        "a refusal is never an ERROR: {refusal:?}"
    );

    let failure = lines_in(&groups[2].0);
    assert!(
        failure.iter().any(|(level, message)| *level == Level::ERROR
            && message == "failed at sign: codesign exited with exit status: 1"),
        "a failed step is an ERROR with the reason: {failure:?}"
    );

    let signed = lines_in(&groups[3].0);
    let messages: Vec<&str> = signed
        .iter()
        .filter(|(level, _)| *level == Level::DEBUG)
        .map(|(_, message)| message.as_str())
        .collect();
    for step in [
        "keychain",
        "sign",
        "notarize",
        "staple",
        "dmg",
        "keychain deleted",
    ] {
        assert!(
            messages.contains(&step),
            "a debug line for {step}: {signed:?}"
        );
    }
    assert!(
        signed.iter().all(|(level, _)| *level == Level::DEBUG),
        "a signed run that succeeds logs only steps: {signed:?}"
    );

    let stuck = lines_in(&groups[4].0);
    assert!(
        stuck.iter().any(|(level, message)| *level == Level::ERROR
            && message.starts_with("could not delete the keychain ")),
        "a failed clean-up is an ERROR: {stuck:?}"
    );

    let text = trace.text.lock().unwrap();
    for secret in [
        "hunter2".to_owned(),
        base64(b"certificate hunter2"),
        base64(b"key hunter2"),
    ] {
        assert!(
            !text.contains(&secret),
            "a secret in the trace: {secret}: {}",
            text.lines()
                .filter(|line| line.contains(&secret))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}
