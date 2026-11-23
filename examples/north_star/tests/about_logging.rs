//! The `north_star` extension handles `north_star::AboutNorthStar` by logging
//! `about North Star` at `INFO`.
//!
//! This test captures `tracing` output, so it is alone in its file.

use gpui::{
    AnyWindowHandle, MenuItem, TestAppContext, VisualTestContext, WindowHandle, WindowOptions, px,
    size,
};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
struct Line {
    target: &'static str,
    level: tracing::Level,
    fields: Vec<(String, String)>,
}

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
    lines: Arc<Mutex<Vec<Line>>>,
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

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        self.lines.lock().unwrap().push(Line {
            target: event.metadata().target(),
            level: *event.metadata().level(),
            fields: fields.0,
        });
    }

    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

fn draw(window: WindowHandle<yard_shell::Shell>, cx: &mut TestAppContext) {
    AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

#[gpui::test]
fn north_star_extension_handles_about_action_and_logs_info(cx: &mut TestAppContext) {
    let product = north_star::application().product;
    let about_action = north_star::menus(&product)
        .into_iter()
        .flat_map(|menu| menu.items)
        .find_map(|item| match item {
            MenuItem::Action { name, action, .. } if name.as_ref() == "About North Star" => {
                Some(action)
            }
            _ => None,
        })
        .expect("north_star::menus includes an `About North Star` action item");
    assert_eq!(about_action.name(), "north_star::AboutNorthStar");

    let (_, init) = north_star::EXTENSIONS
        .iter()
        .copied()
        .find(|(name, _)| *name == "north_star")
        .expect("north_star::EXTENSIONS includes `north_star`");

    cx.update(yard_ui::init);

    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        cx.update(|cx| {
            let extension = yard_shell::Extension::new(cx, "north_star")
                .expect("`north_star` extension handle is created");
            init(cx, &extension);
        });
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), yard_shell::new_shell)
                .unwrap()
        });
        cx.simulate_window_resize(window.into(), size(px(640.), px(480.)));
        draw(window, cx);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| window.dispatch_action(about_action, cx));
        draw(window, cx);
    });

    let lines = trace.lines.lock().unwrap();
    let errors: Vec<&Line> = lines
        .iter()
        .filter(|line| line.level == tracing::Level::ERROR)
        .collect();
    assert!(
        errors.is_empty(),
        "dispatching AboutNorthStar logged tracing ERRORs: {errors:#?}"
    );

    let about_lines: Vec<&Line> = lines
        .iter()
        .filter(|line| {
            line.target.starts_with("north_star")
                && line.level == tracing::Level::INFO
                && line
                    .fields
                    .iter()
                    .any(|(key, value)| key == "message" && value == "about North Star")
        })
        .collect();
    assert_eq!(
        about_lines.len(),
        1,
        "expected one `INFO` line `about North Star` from `north_star`, got: {lines:#?}"
    );
}
