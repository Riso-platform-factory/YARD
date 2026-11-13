use gpui::{AnyWindowHandle, TestAppContext};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Captured {
    spans: BTreeMap<u64, String>,
    entered: Vec<u64>,
    lines: Vec<String>,
    span_stacks: Vec<Vec<String>>,
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
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        let label = format!("{}{}", attributes.metadata().name(), fields.0);
        let target = attributes.metadata().target();
        if target.starts_with("riso_shell") || target.starts_with("window_tour") {
            let mut stack: Vec<String> = captured
                .entered
                .iter()
                .map(|entered| captured.spans[entered].clone())
                .collect();
            stack.push(label.clone());
            captured.span_stacks.push(stack);
        }
        captured.spans.insert(id, label);
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let target = event.metadata().target();
        if !target.starts_with("riso_shell")
            && !target.starts_with("riso_layout")
            && !target.starts_with("window_tour")
        {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        let group = captured
            .entered
            .last()
            .map(|id| captured.spans[id].as_str())
            .unwrap_or("none");
        let line = format!("{group}: {}{}", event.metadata().level(), fields.0);
        captured.lines.push(line);
    }

    fn enter(&self, id: &tracing::span::Id) {
        self.0.lock().unwrap().entered.push(id.into_u64());
    }

    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(self.0.lock().unwrap().entered.pop(), Some(id.into_u64()));
    }
}

static DIAGNOSTICS: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("window_tour") || metadata.target().starts_with("riso_shell")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            DIAGNOSTICS
                .lock()
                .unwrap()
                .push(format!("{} {}", record.level(), record.args()));
        }
    }

    fn flush(&self) {}
}

fn run_child(case: Option<&str>) {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "title_bar_case_places_three_nodes_after_window_frame",
            "--nocapture",
        ])
        .env("WINDOW_TOUR_TEST_CHILD", case.unwrap_or("unset"));
    match case {
        Some(value) => {
            command.env("WINDOW_TOUR", value);
        }
        None => {
            command.env_remove("WINDOW_TOUR");
        }
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "child {case:?} failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[gpui::test]
fn title_bar_case_places_three_nodes_after_window_frame(cx: &mut TestAppContext) {
    let Ok(mode) = std::env::var("WINDOW_TOUR_TEST_CHILD") else {
        run_child(None);
        run_child(Some(""));
        run_child(Some("title-bar"));
        run_child(Some("unknown"));
        return;
    };

    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    cx.update(riso_ui::init);

    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        cx.update(|cx| {
            let ext = riso_shell::Extension::new(cx, "window_tour").unwrap();
            window_tour::init(cx, &ext);
        });
        if mode == "title-bar" {
            let window = cx.update(|cx| {
                cx.open_window(
                    gpui::WindowOptions {
                        titlebar: Some(riso_ui::titlebar_options("riso")),
                        ..Default::default()
                    },
                    riso_shell::new_shell,
                )
                .unwrap()
            });
            cx.simulate_window_resize(window.into(), gpui::size(gpui::px(800.), gpui::px(600.)));
            let handle = AnyWindowHandle::from(window);
            handle
                .update(cx, |_, window, cx| {
                    window.refresh();
                    let _ = window.draw(cx);
                })
                .unwrap();
            let mut visual = gpui::VisualTestContext::from_window(handle, cx);
            let ping = visual
                .debug_bounds("BAR-PING")
                .expect("ping button is drawn in the title bar");
            visual.simulate_click(ping.center(), Default::default());
        }
    });

    let captured = trace.0.lock().unwrap();
    let diagnostics = DIAGNOSTICS.lock().unwrap();

    // Every extension creation places the shell's window-frame group first.
    for frame_span in [
        "root-place node=\"window\" component=\"riso.window\"",
        "node-place node=\"title_bar\" component=\"riso.title_bar\" parent=\"window\" slot=\"title_bar\"",
        "node-place node=\"body\" component=\"riso.body\" parent=\"window\" slot=\"body\"",
    ] {
        assert!(
            captured
                .span_stacks
                .contains(&vec!["window-frame".to_string(), frame_span.to_string()]),
            "missing `window-frame > {frame_span}` in {:?}",
            captured.span_stacks
        );
    }

    match mode.as_str() {
        "unset" | "" => {
            assert!(
                !captured
                    .lines
                    .iter()
                    .any(|line| line.contains("extension=window_tour")),
                "expected no window_tour placements when WINDOW_TOUR={mode:?}, got {:?}",
                captured.lines
            );
            assert!(
                diagnostics.is_empty(),
                "unexpected diagnostics: {diagnostics:?}"
            );
        }
        "title-bar" => {
            for expected in [
                "root-place node=\"root\" component=\"window_tour.card\": DEBUG message=node placed node=root component=window_tour.card extension=window_tour parent=body slot=children",
                "node-place node=\"title\" component=\"window_tour.title\" parent=\"title_bar\" slot=\"left\": DEBUG message=node placed node=title component=window_tour.title extension=window_tour parent=title_bar slot=left",
                "node-place node=\"ping\" component=\"window_tour.ping\" parent=\"title_bar\" slot=\"right\": DEBUG message=node placed node=ping component=window_tour.ping extension=window_tour parent=title_bar slot=right",
                "button-click id=ping: INFO message=ping",
            ] {
                assert!(
                    captured.lines.iter().any(|line| line == expected),
                    "missing `{expected}` in {:#?}",
                    captured.lines
                );
            }
            assert!(
                diagnostics.is_empty(),
                "unexpected diagnostics: {diagnostics:?}"
            );
        }
        "unknown" => {
            assert_eq!(
                diagnostics.len(),
                1,
                "expected one warning: {diagnostics:?}"
            );
            assert!(
                diagnostics[0].starts_with("WARN ") && diagnostics[0].contains("unknown"),
                "unexpected diagnostic: {}",
                diagnostics[0]
            );
        }
        other => panic!("unexpected mode {other}"),
    }
}
