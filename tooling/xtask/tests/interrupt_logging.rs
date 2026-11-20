#![cfg(all(feature = "dev", target_os = "macos"))]
//! An interrupt is logged at `info` inside the `bundle-macos` group when the
//! run sees it, the run stops at its next step and logs that at `info`, and
//! nothing about it is an `ERROR`, even when the running child fails of it.
//!
//! This test captures `tracing` output and starts `yard_log`, which installs
//! a process-wide `log` logger, so it is alone in its file and no other test
//! in this binary reaches the same log lines.

#[allow(dead_code)]
mod support;

use std::sync::{Arc, Mutex};
use support::{FakeRunner, base64, signed_config, tempdir};
use tracing::Level;
use xtask::{Interrupt, MacosArgs, bundle_macos};

/// The lines logged, each with whether it was logged inside the
/// `bundle-macos` group.
#[derive(Clone, Default)]
struct Trace {
    lines: Arc<Mutex<Vec<(bool, Level, String)>>>,
    groups: Arc<Mutex<Vec<&'static str>>>,
    open: Arc<Mutex<Vec<tracing::span::Id>>>,
}

#[derive(Default)]
struct Message(String);
impl tracing::field::Visit for Message {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.into();
        }
    }
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl tracing::Subscriber for Trace {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut groups = self.groups.lock().unwrap();
        groups.push(attributes.metadata().name());
        tracing::span::Id::from_u64(groups.len() as u64)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut message = Message::default();
        event.record(&mut message);
        let in_group = self.open.lock().unwrap().last().is_some_and(|id| {
            self.groups.lock().unwrap()[id.into_u64() as usize - 1] == "bundle-macos"
        });
        self.lines
            .lock()
            .unwrap()
            .push((in_group, *event.metadata().level(), message.0));
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
fn a_set_interrupt_flag_is_logged_at_info_and_never_as_an_error() {
    start_logs();
    let temp = tempdir();
    let config = signed_config(temp.path(), true, true);
    let options = MacosArgs {
        config: Some(config),
        out: Some(temp.path().join("out")),
        ..MacosArgs::default()
    };
    let secrets = xtask::Secrets {
        certificate: Some(base64(b"certificate")),
        certificate_password: Some("password".into()),
        notarization_key: Some(base64(b"key")),
        notarization_key_id: Some("KEYID".into()),
        notarization_issuer_id: Some("ISSUER".into()),
    };
    let interrupt = Interrupt::default();
    let mut runner = FakeRunner::new(temp.path());
    let during_codesign = interrupt.clone();
    runner.on_verify = Some(Box::new(move |_| during_codesign.set()));

    let trace = Trace::default();
    let failure = tracing::subscriber::with_default(trace.clone(), || {
        bundle_macos(&options, &secrets, &interrupt, &mut runner).unwrap_err()
    });
    assert_eq!(
        (failure.step, failure.reason.as_str()),
        ("notarize", "interrupted")
    );

    let lines = trace.lines.lock().unwrap();
    let interrupted = lines
        .iter()
        .position(|line| {
            *line
                == (
                    true,
                    Level::INFO,
                    "interrupted; stopping after the current step".to_owned(),
                )
        })
        .unwrap_or_else(|| panic!("an info line for the interrupt: {lines:?}"));
    assert!(
        !lines.iter().any(|(_, level, _)| *level == Level::ERROR),
        "an interrupt is never an ERROR: {lines:?}"
    );
    assert!(
        lines.contains(&(true, Level::INFO, "stopped: interrupted".to_owned())),
        "the run's stop is an info line in the group: {lines:?}"
    );
    let after: Vec<&str> = lines[interrupted + 1..]
        .iter()
        .map(|(_, _, message)| message.as_str())
        .collect();
    assert_eq!(
        after,
        [
            "keychain clean-up",
            "keychain deleted",
            "private folder removed",
            "stopped: interrupted",
        ],
        "no step runs after the interrupt, only the clean-up"
    );

    drop(lines);

    // Ctrl-C reaches the running `codesign` too, which fails of it: still an
    // interrupt, logged at info with the child's reason.
    let interrupt = Interrupt::default();
    let mut runner = FakeRunner::new(temp.path());
    runner.fail = Some("codesign");
    let signal = interrupt.clone();
    runner.on_fail = Some(Box::new(move || signal.set()));
    let trace = Trace::default();
    let failure = tracing::subscriber::with_default(trace.clone(), || {
        bundle_macos(&options, &secrets, &interrupt, &mut runner).unwrap_err()
    });
    assert_eq!(failure.step, "sign");
    let lines = trace.lines.lock().unwrap();
    assert!(
        !lines.iter().any(|(_, level, _)| *level == Level::ERROR),
        "a child stopped by the interrupt is never an ERROR: {lines:?}"
    );
    assert_eq!(
        lines.last(),
        Some(&(true, Level::INFO, "stopped: interrupted".to_owned())),
        "{lines:?}"
    );
}
