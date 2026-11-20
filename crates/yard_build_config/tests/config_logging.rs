#![cfg(feature = "dev")]
//! `read` runs inside the group `build-config-read{file}`: each accepted
//! value at `debug`, and a refusal at `info`, never as an `ERROR`.
//!
//! This test captures `tracing` output and is alone in its file, so no other
//! test in this binary reaches the same log lines.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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

/// A config under test in `tests/configs/`.
fn under_test(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/configs")
        .join(name)
        .join("product.toml")
}

#[test]
fn reading_runs_inside_the_build_config_read_group() {
    let plain = under_test("plain");
    let bad = under_test("bad-identifier");
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        yard_build_config::read(&plain).unwrap();
        yard_build_config::read(&bad).unwrap_err();
    });
    let records = trace.records.lock().unwrap();

    // The group `build-config-read{file}` of the read of `path`.
    let group = |path: &Path| -> tracing::span::Id {
        records
            .iter()
            .find_map(|record| match record {
                Record::Group { id, name, fields } if *name == "build-config-read" => {
                    assert_eq!(
                        fields
                            .iter()
                            .map(|(key, _)| key.as_str())
                            .collect::<Vec<_>>(),
                        ["file"],
                        "the `build-config-read` group's fields"
                    );
                    fields
                        .iter()
                        .any(|(key, value)| key == "file" && *value == path.display().to_string())
                        .then(|| id.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no `build-config-read` group for {path:?}: {records:?}"))
    };
    let lines_in = |id: &tracing::span::Id| -> Vec<(tracing::Level, String)> {
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
    assert!(
        records
            .iter()
            .all(|record| !matches!(record, Record::Line { group: None, .. })),
        "every line is inside a group: {records:?}"
    );

    let accepted = lines_in(&group(&plain));
    assert_eq!(
        accepted,
        [
            "name",
            "version",
            "identifier",
            "url scheme",
            "icon",
            "update url"
        ]
        .map(|message| (tracing::Level::DEBUG, message.to_owned())),
        "each accepted value is a debug line"
    );

    let refused = lines_in(&group(&bad));
    assert!(
        refused
            .iter()
            .any(|(level, message)| *level == tracing::Level::INFO
                && message.starts_with("refused: ")
                && message.contains("`identifier`")),
        "a refusal is an info line with the reason: {refused:?}"
    );
    assert!(
        !refused
            .iter()
            .any(|(level, _)| *level == tracing::Level::ERROR),
        "a refusal is never an ERROR: {refused:?}"
    );
}
