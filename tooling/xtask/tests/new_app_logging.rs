#![cfg(feature = "dev")]
//! `new_app` runs inside the group `new-app{name, identifier}`: a `debug` line
//! per step (`checked`, `written`, `renamed`, `workspace`, `lock`) in order on
//! success, and `refused: <reason>` at `INFO` with no `ERROR` on a refusal.
//!
//! This test captures `tracing` output and starts `yard_log`, which installs a
//! process-wide `log` logger, so it is alone in its file and no other test in
//! this binary reaches the same log lines.

#[allow(dead_code)]
mod support;

use std::fs;
use std::sync::{Arc, Mutex};
use support::{TempDir, tempdir};
use tracing::Level;
use xtask::{NewAppArgs, new_app};

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
        level: Level,
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
/// output off, so diagnostic lines (`log`) reach the test's subscriber as they
/// reach the lifecycle trace.
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

fn workspace() -> TempDir {
    let temp = tempdir();
    let root = temp.path();
    fs::write(
        root.join("Cargo.toml"),
        support::new_app_manifest("[workspace]\nresolver = \"3\"\nmembers = [\"app/*\", \"crates/*\"]\ndefault-members = [\"app/yard\"]\n"),
    )
    .unwrap();
    let yard = root.join("app/yard");
    fs::create_dir_all(&yard).unwrap();
    fs::write(
        yard.join("Cargo.toml"),
        "[package]\nname = \"yard\"\nversion = \"0.1.0\"\nedition.workspace = true\n",
    )
    .unwrap();
    let real_yard = xtask::repository().join("app/yard");
    fs::copy(real_yard.join("product.toml"), yard.join("product.toml")).unwrap();
    fs::copy(real_yard.join("icon.png"), yard.join("icon.png")).unwrap();
    let yard_ui = root.join("crates/yard_ui");
    fs::create_dir_all(&yard_ui).unwrap();
    fs::write(
        yard_ui.join("Cargo.toml"),
        "[package]\nname = \"yard_ui\"\nversion = \"0.1.0\"\nedition.workspace = true\n",
    )
    .unwrap();
    support::new_app_workspace_targets(root);
    temp
}

#[test]
fn new_app_runs_inside_the_new_app_group() {
    start_logs();

    let ok_workspace = workspace();
    let bad_workspace = workspace();
    let ok_args = NewAppArgs {
        name: "North Star".into(),
        identifier: "com.acme.northstar".into(),
        ..NewAppArgs::default()
    };
    let bad_args = NewAppArgs {
        name: "North Star".into(),
        identifier: "not-an-identifier".into(),
        ..NewAppArgs::default()
    };

    let trace = Trace::default();
    let refusal = tracing::subscriber::with_default(trace.clone(), || {
        new_app(ok_workspace.path(), &ok_args).unwrap();
        new_app(bad_workspace.path(), &bad_args).unwrap_err()
    });
    let records = trace.records.lock().unwrap();

    let groups: Vec<(tracing::span::Id, Vec<(String, String)>)> = records
        .iter()
        .filter_map(|record| match record {
            Record::Group { id, name, fields } if *name == "new-app" => {
                Some((id.clone(), fields.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        groups.len(),
        2,
        "expected one new-app group per run: {records:?}"
    );

    let field_map = |fields: &[(String, String)]| -> std::collections::BTreeMap<String, String> {
        fields.iter().cloned().collect()
    };
    let field = |key: &str, value: &str| (key.to_owned(), value.to_owned());
    assert_eq!(groups[0].1.len(), 2, "success group field count");
    assert_eq!(
        field_map(&groups[0].1),
        std::collections::BTreeMap::from([
            field("name", "North Star"),
            field("identifier", "com.acme.northstar"),
        ]),
        "success group fields"
    );
    assert_eq!(groups[1].1.len(), 2, "refusal group field count");
    assert_eq!(
        field_map(&groups[1].1),
        std::collections::BTreeMap::from([
            field("name", "North Star"),
            field("identifier", "not-an-identifier"),
        ]),
        "refusal group fields"
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

    // Success run: five debug steps in order, no ERROR.
    let ok_lines = lines_in(&groups[0].0);
    let debug_messages: Vec<&str> = ok_lines
        .iter()
        .filter(|(level, _)| *level == Level::DEBUG)
        .map(|(_, message)| message.as_str())
        .collect();
    let expected_steps = ["checked", "written", "renamed", "workspace", "lock"];
    let mut cursor = 0;
    for step in expected_steps {
        let found = debug_messages[cursor..]
            .iter()
            .position(|message| *message == step)
            .unwrap_or_else(|| {
                panic!(
                    "expected DEBUG step `{step}` in order among {debug_messages:?} (all lines: {ok_lines:?})"
                )
            });
        cursor += found + 1;
    }

    // Refusal run: `refused: <reason>` at INFO inside the group, no ERROR anywhere.
    let refusal_lines = lines_in(&groups[1].0);
    let expected_refusal = format!("refused: {}", refusal.reason);
    assert!(
        refusal_lines
            .iter()
            .any(|(level, message)| *level == Level::INFO && *message == expected_refusal),
        "expected INFO `{expected_refusal}` inside new-app group: {refusal_lines:?}"
    );
    assert!(
        !records.iter().any(|record| matches!(
            record,
            Record::Line {
                level: Level::ERROR,
                ..
            }
        )),
        "neither a success nor a refusal may log an ERROR: {records:?}"
    );
}
