#![cfg(feature = "dev")]

use gpui::{
    AppContext, InteractiveElement, IntoElement, ScrollDelta, ScrollEvent, Styled, TestAppContext,
    point, px,
};
use riso_shell::dev as shell;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Once},
};

// One process-wide diagnostic logger, installed once: this file's one test
// never sees another test's records.
static RECORDS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());
static INIT: Once = Once::new();

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso_shell")
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            RECORDS
                .lock()
                .unwrap()
                .push((record.level(), record.args().to_string()));
        }
    }
    fn flush(&self) {}
}

fn diagnostics() {
    INIT.call_once(|| {
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
}

fn errors() -> Vec<String> {
    RECORDS
        .lock()
        .unwrap()
        .iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .map(|(_, message)| message.clone())
        .collect()
}

#[derive(Default)]
struct Captured {
    spans: BTreeMap<u64, String>,
    entered: Vec<u64>,
    lines: Vec<String>,
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
        captured
            .spans
            .insert(id, format!("{}{}", attributes.metadata().name(), fields.0));
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("riso_shell") {
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

fn lines_with(trace: &Trace, text: &str) -> Vec<String> {
    trace
        .0
        .lock()
        .unwrap()
        .lines
        .iter()
        .filter(|line| line.contains(text))
        .cloned()
        .collect()
}

// The window is narrow: twenty `tNN` tabs cannot fit in its width.
const WINDOW_WIDTH: f32 = 480.;
const TABS: [&str; 20] = [
    "t01", "t02", "t03", "t04", "t05", "t06", "t07", "t08", "t09", "t10", "t11", "t12", "t13",
    "t14", "t15", "t16", "t17", "t18", "t19", "t20",
];

// A card fills the zone its container gives it, found as `CARD-<node>`.
struct Card {
    node: String,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .debug_selector(|| format!("CARD-{}", self.node))
            .size_full()
    }
}

fn draw(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn open_and_draw(app: &mut TestAppContext) -> gpui::WindowHandle<shell::Shell> {
    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(px(WINDOW_WIDTH), px(360.)));
    draw(window, app);
    window
}

// A tabs container over twenty cards, placed before the window opens so they
// are the default layout; the cards' tabs are `TAB-<node>`.
fn setup(app: &mut TestAppContext) -> shell::Extension {
    app.update(|cx| {
        riso_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        host.register(cx, "tour.card", &[], |node, _, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
            })
            .into()
        });
        host.place_root(cx, "tabs", "riso.tabs");
        for node in TABS {
            host.place(cx, node, "tour.card", "tabs", "children");
        }
        host
    })
}

fn selected(visual: &mut gpui::VisualTestContext) -> Option<String> {
    visual.update(|_, cx| shell::selected_tab(cx, "tabs").map(|node| node.to_string()))
}

fn click(visual: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} has bounds"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
}

// Alone in this binary: every call reaching the captured lines uses the one
// subscriber.
#[gpui::test]
fn tab_scrolled_into_view_is_logged_once_and_only_when_the_row_moves(cx: &mut TestAppContext) {
    diagnostics();
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        let _host = setup(&mut app);
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        // Selecting a tab that is in view does not move the row.
        click(&mut visual, "TAB-t02");
        draw(window, &mut app);
        assert_eq!(selected(&mut visual).as_deref(), Some("t02"));
        assert!(
            lines_with(&trace, "tab scrolled into view").is_empty(),
            "no `tab scrolled into view` while the row does not move: {:?}",
            lines_with(&trace, "tab scrolled into view")
        );
        // Select t20 through the container's focus: the row moves to show it.
        click(&mut visual, "CARD-t02");
        draw(window, &mut app);
        for _ in 1..TABS.len() - 1 {
            visual.dispatch_action(shell::SelectNextTab);
        }
        draw(window, &mut app);
        assert_eq!(selected(&mut visual).as_deref(), Some("t20"));
        // Selecting t19, in view once t20 is shown, does not move the row.
        click(&mut visual, "TAB-t19");
        draw(window, &mut app);
        assert_eq!(selected(&mut visual).as_deref(), Some("t19"));
        assert!(
            lines_with(&trace, "tab bar scrolled").is_empty(),
            "scrolls into view are not scrolls by the user: {:?}",
            lines_with(&trace, "tab bar scrolled")
        );
        // The wheel over the bar is the user's scroll.
        let over = visual
            .debug_bounds("TAB-t19")
            .expect("t19's tab draws")
            .center();
        visual.simulate_event(ScrollEvent {
            position: over,
            delta: ScrollDelta::Pixels(point(px(40.), px(0.))),
            ..Default::default()
        });
        draw(window, &mut app);
        assert!(
            !lines_with(&trace, "tab bar scrolled").is_empty(),
            "the wheel over the bar logs `tab bar scrolled`"
        );
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    let into_view: Vec<&String> = captured
        .lines
        .iter()
        .filter(|line| line.contains("tab scrolled into view"))
        .collect();
    // `dispatch_action` draws a frame after each action, as the app does
    // between key presses: each step that moves the row logs one line.
    assert!(
        !into_view.is_empty(),
        "a step that moves the row logs `tab scrolled into view`: {:#?}",
        captured.lines
    );
    assert!(
        into_view
            .iter()
            .all(|line| line.contains("DEBUG") && line.contains("tabs=tabs")),
        "every `tab scrolled into view` is a debug naming the container: {into_view:?}"
    );
    let nodes: Vec<&str> = into_view
        .iter()
        .map(|line| {
            line.split_whitespace()
                .filter_map(|field| field.strip_prefix("node="))
                .next_back()
                .unwrap_or_else(|| panic!("`tab scrolled into view` names a tab: {line}"))
        })
        .collect();
    let order: Vec<usize> = nodes
        .iter()
        .map(|node| {
            TABS[2..]
                .iter()
                .position(|tab| tab == node)
                .unwrap_or_else(|| panic!("{node} is a tab the stepping went to"))
        })
        .collect();
    assert!(
        order.windows(2).all(|pair| pair[0] < pair[1]),
        "the lines' nodes are the tabs in the order they were stepped to: {nodes:?}"
    );
    assert!(
        into_view.last().unwrap().contains("node=t20"),
        "the last `tab scrolled into view` names t20: {}",
        into_view.last().unwrap()
    );
    drop(captured);
    let scrolled = lines_with(&trace, "tab bar scrolled");
    assert!(
        scrolled.iter().all(|line| line.contains("TRACE")),
        "`tab bar scrolled` is a trace: {scrolled:?}"
    );
    assert!(
        scrolled
            .iter()
            .all(|line| line.contains("tabs=tabs") && line.contains("offset=")),
        "`tab bar scrolled` names the container and the offset: {scrolled:?}"
    );
    assert!(errors().is_empty(), "{:?}", errors());
}
