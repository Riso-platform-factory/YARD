#![cfg(feature = "dev")]

use gpui::{AppContext, TestAppContext};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Captured {
    lines: Vec<String>,
    spans: BTreeMap<u64, String>,
    entered: Vec<u64>,
    creation_fields: BTreeMap<u64, String>,
    parents: Vec<(String, Option<String>)>,
}
#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Captured>>);
#[derive(Default)]
struct Fields(String);
impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        write!(self.0, " {}={value:?}", field.name()).unwrap();
    }
}
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
        let mut captured = self.0.lock().unwrap();
        let id = captured.spans.len() as u64 + 1;
        captured
            .spans
            .insert(id, attributes.metadata().name().into());
        if attributes.metadata().target().starts_with("riso_shell") {
            let mut fields = Fields::default();
            attributes.record(&mut fields);
            if attributes.metadata().name() == "component-create" {
                let parent = captured
                    .entered
                    .iter()
                    .rev()
                    .find_map(|id| captured.creation_fields.get(id).cloned());
                captured.parents.push((fields.0.clone(), parent));
                captured.creation_fields.insert(id, fields.0.clone());
            }
            captured.lines.push(format!(
                "{} {}{}",
                attributes.metadata().level(),
                attributes.metadata().name(),
                fields.0
            ));
        }
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if *event.metadata().level() == tracing::Level::TRACE {
            return;
        }
        let target = event.metadata().target();
        if !target.starts_with("riso_shell")
            && !target.starts_with("riso_layout")
            && target != "component_diagnostic"
        {
            return;
        }
        let mut captured = self.0.lock().unwrap();
        let group = captured
            .entered
            .last()
            .map(|id| captured.spans[id].clone())
            .expect("operation event inside a group");
        let mut fields = Fields::default();
        event.record(&mut fields);
        captured.lines.push(format!(
            "  {group}: {}{}",
            event.metadata().level(),
            fields.0
        ));
    }
    fn enter(&self, id: &tracing::span::Id) {
        self.0.lock().unwrap().entered.push(id.into_u64());
    }
    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(self.0.lock().unwrap().entered.pop(), Some(id.into_u64()));
    }
}

struct Diagnostics;
impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso_shell")
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            assert_eq!(record.level(), log::Level::Error);
            tracing::error!(target: "component_diagnostic", "{}", record.args());
        }
    }
    fn flush(&self) {}
}

#[gpui::test]
fn splits_are_built_in_and_log_the_shown_tree(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    cx.update(riso_ui::init);
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        cx.update(|cx| {
            let _group = tracing::info_span!("extension-init", extension = "tour").entered();
            assert!(riso_shell::dev::Extension::new(cx, "riso").is_none());
            let host = riso_shell::dev::Extension::new(cx, "tour").unwrap();
            let guest = riso_shell::dev::Extension::new(cx, "guest").unwrap();
            host.register(cx, "riso.badge", &[], |_, _, _, _| {
                panic!("reserved registration accepted")
            });
            host.register(cx, "riso.row", &[], |_, _, _, _| {
                panic!("built-in replaced")
            });
            host.register(cx, "tour.card", &[], |_, _, _, cx| {
                cx.new(|_| gpui::Empty).into()
            });
            guest.place_root(cx, "root", "riso.row");
            host.place(cx, "right", "riso.column", "root", "children");
            host.place(cx, "card", "tour.card", "right", "children");
            guest.place(cx, "empty", "riso.row", "right", "children");
            host.place(cx, "missing", "tour.missing", "root", "children");
        });
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), riso_shell::dev::new_shell)
                .unwrap()
        });
        gpui::AnyWindowHandle::from(window)
            .update(cx, |_, window, cx| {
                window.refresh();
                let _ = window.draw(cx);
            })
            .unwrap();
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    for line in [
        "  extension-init: ERROR message=riso is the shell's own namespace",
        "  component-register: ERROR message=extension tour cannot register component riso.badge",
        "  component-register: ERROR message=extension tour cannot register component riso.row",
        "  component-create: DEBUG message=split created axis=\"row\" children=0",
        "  component-create: DEBUG message=split created axis=\"column\" children=2",
        "  component-create: DEBUG message=split created axis=\"row\" children=1",
    ] {
        assert!(
            captured.lines.iter().any(|actual| actual == line),
            "missing {line:?} in {:#?}",
            captured.lines
        );
    }
    assert_eq!(
        captured
            .lines
            .iter()
            .filter(|line| line.contains("split created"))
            .count(),
        3
    );
    let root = " node=root component=riso.row extension=guest";
    let right = " node=right component=riso.column extension=tour";
    assert_eq!(
        captured.parents,
        [
            (root.into(), None),
            (right.into(), Some(root.into())),
            (
                " node=card component=tour.card extension=tour".into(),
                Some(right.into())
            ),
            (
                " node=empty component=riso.row extension=guest".into(),
                Some(right.into())
            ),
            (
                " node=missing component=tour.missing extension=tour".into(),
                Some(root.into())
            ),
        ]
    );
}
