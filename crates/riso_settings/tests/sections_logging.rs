use gpui::TestAppContext;
use riso_settings::{Settings, get, init, register};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};

struct Captured {
    next_id: u64,
    spans: BTreeMap<u64, &'static str>,
    entered: Vec<u64>,
    entered_groups: BTreeSet<u64>,
    groups: Vec<(u64, String)>,
    events: Vec<(Option<u64>, String)>,
    diagnostics: Vec<(log::Level, Option<u64>, String)>,
}

impl Captured {
    const fn new() -> Self {
        Self {
            next_id: 1,
            spans: BTreeMap::new(),
            entered: Vec::new(),
            entered_groups: BTreeSet::new(),
            groups: Vec::new(),
            events: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn current_settings_group(&self) -> Option<u64> {
        self.entered
            .iter()
            .rev()
            .copied()
            .find(|id| self.spans[id].starts_with("riso_settings"))
    }
}

static CAPTURED: Mutex<Captured> = Mutex::new(Captured::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso_settings")
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        let mut captured = CAPTURED.lock().unwrap();
        let group = captured.current_settings_group();
        captured.diagnostics.push((record.level(), group, message));
    }

    fn flush(&self) {}
}

#[derive(Default)]
struct Fields(String);

impl Fields {
    fn push(&mut self, name: &str, value: &str) {
        use std::fmt::Write;
        write!(self.0, " {name}={value}").unwrap();
    }
}

impl tracing::field::Visit for Fields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.push(field.name(), value);
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.push(field.name(), &format!("{value:?}"));
    }
}

struct Trace;

impl tracing::Subscriber for Trace {
    fn register_callsite(
        &self,
        _: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::sometimes()
    }

    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        let metadata = attributes.metadata();
        let target = metadata.target();
        let label = format!("{} {}{}", metadata.level(), metadata.name(), fields.0);
        let mut captured = CAPTURED.lock().unwrap();
        let id = captured.next_id;
        captured.next_id += 1;
        if target.starts_with("riso_settings") {
            captured.groups.push((id, label));
        }
        captured.spans.insert(id, target);
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let metadata = event.metadata();
        if !metadata.target().starts_with("riso_settings") {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let line = format!("{}{}", metadata.level(), fields.0);
        let mut captured = CAPTURED.lock().unwrap();
        let group = captured.current_settings_group();
        captured.events.push((group, line));
    }

    fn enter(&self, id: &tracing::span::Id) {
        let id = id.into_u64();
        let mut captured = CAPTURED.lock().unwrap();
        if captured.spans[&id].starts_with("riso_settings") {
            captured.entered_groups.insert(id);
        }
        captured.entered.push(id);
    }

    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(CAPTURED.lock().unwrap().entered.pop(), Some(id.into_u64()));
    }
}

#[derive(serde::Deserialize, PartialEq, Debug)]
struct First {
    greeting: String,
    size: u32,
}

impl Default for First {
    fn default() -> Self {
        Self {
            greeting: "hello".into(),
            size: 14,
        }
    }
}

impl Settings for First {
    const KEY: &'static str = "first";
}

#[derive(serde::Deserialize, PartialEq, Debug)]
struct Second {
    enabled: bool,
}

impl Default for Second {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl Settings for Second {
    const KEY: &'static str = "second";
}

#[derive(serde::Deserialize, Default, PartialEq, Debug)]
struct FirstDuplicate {}

impl Settings for FirstDuplicate {
    const KEY: &'static str = First::KEY;
}

// Alone in this binary: installs the process-wide diagnostic logger and captures every callsite.
#[gpui::test]
fn sections_operations_open_groups_and_log_every_contract_line(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);

    tracing::subscriber::with_default(Trace, || {
        cx.update(|cx| {
            init(cx);
            register::<First>(cx);
            register::<Second>(cx);
            register::<First>(cx);
            register::<FirstDuplicate>(cx);
        });
    });

    cx.read(|cx| {
        let first = get::<First>(cx);
        assert_eq!(first.greeting, "hello");
        assert_eq!(first.size, 14);
        let second = get::<Second>(cx);
        assert!(second.enabled);
    });

    let captured = CAPTURED.lock().unwrap();
    assert!(
        captured.entered.is_empty(),
        "spans still entered: {:?}",
        captured.entered
    );
    let [
        (init_id, ref init_group),
        (first_id, ref first_group),
        (second_id, ref second_group),
        (dup_same_id, ref dup_same_group),
        (dup_other_id, ref dup_other_group),
    ] = captured.groups[..]
    else {
        panic!("expected 5 groups, got {:#?}", captured.groups);
    };
    assert_eq!(init_group, "INFO settings-init");
    assert_eq!(first_group, "INFO settings-register section=first");
    assert_eq!(second_group, "INFO settings-register section=second");
    assert_eq!(dup_same_group, "INFO settings-register section=first");
    assert_eq!(dup_other_group, "INFO settings-register section=first");

    for id in [init_id, first_id, second_id, dup_same_id, dup_other_id] {
        assert!(
            captured.entered_groups.contains(&id),
            "group {id} was opened but never entered"
        );
    }

    assert_eq!(
        captured.events,
        [
            (Some(init_id), "DEBUG message=store created".to_string()),
            (Some(first_id), "DEBUG message=registered".to_string()),
            (Some(second_id), "DEBUG message=registered".to_string()),
        ],
        "{:#?}",
        captured.events
    );

    assert_eq!(
        captured.diagnostics,
        [
            (
                log::Level::Error,
                Some(dup_same_id),
                "section first is already registered".to_string(),
            ),
            (
                log::Level::Error,
                Some(dup_other_id),
                "section first is already registered".to_string(),
            ),
        ],
        "{:#?}",
        captured.diagnostics
    );
}
