//! `Product::new` checks the identifier inside the group `product{name,
//! version, identifier}`: `checked` at `debug`, and an identifier of the wrong
//! form is an `ERROR` there.
//!
//! This test captures `tracing` output and is alone in its file, so no other
//! test in this binary reaches the same log lines.

use riso_product::Product;
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

#[test]
fn checking_the_identifier_runs_inside_the_product_group() {
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        Product::new("sample", "9.9.9", "com.example.sample");
        Product::new("sample", "9.9.9", "sample");
    });
    let records = trace.records.lock().unwrap();

    // The group `product{name, version, identifier}` of the `Product::new`
    // that got `identifier`.
    let group = |identifier: &str| -> tracing::span::Id {
        records
            .iter()
            .find_map(|record| match record {
                Record::Group { id, name, fields } if *name == "product" => {
                    assert_eq!(
                        fields
                            .iter()
                            .map(|(key, _)| key.as_str())
                            .collect::<Vec<_>>(),
                        ["name", "version", "identifier"],
                        "the `product` group's fields"
                    );
                    (fields
                        .iter()
                        .any(|(key, value)| key == "name" && value == "sample")
                        && fields
                            .iter()
                            .any(|(key, value)| key == "version" && value == "9.9.9")
                        && fields
                            .iter()
                            .any(|(key, value)| key == "identifier" && value == identifier))
                    .then(|| id.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!("no `product` group for identifier {identifier:?}: {records:?}")
            })
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

    let accepted = lines_in(&group("com.example.sample"));
    assert!(
        accepted
            .iter()
            .any(|(level, message)| *level == tracing::Level::DEBUG && message == "checked"),
        "the `product` group logs `checked` at debug: {accepted:?}"
    );
    assert!(
        !accepted
            .iter()
            .any(|(level, _)| *level == tracing::Level::ERROR),
        "an identifier of the right form logs no error: {accepted:?}"
    );
    let refused = lines_in(&group("sample"));
    assert!(
        refused
            .iter()
            .any(|(level, _)| *level == tracing::Level::ERROR),
        "an identifier of the wrong form is an ERROR in the `product` group: {refused:?}"
    );
}
