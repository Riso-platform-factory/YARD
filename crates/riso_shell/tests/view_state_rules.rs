#![cfg(feature = "dev")]

use gpui::{AppContext, InteractiveElement, IntoElement, SharedString, Styled, TestAppContext, px};
use riso_layout::NodeId;
use riso_shell::dev as shell;
use serde_json::json;
use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    sync::{Arc, Mutex, Once},
    time::Duration,
};

// Every test of this file captures the lifecycle trace and shares one
// process-wide diagnostic logger, so they all take one lock: no test reaches
// the shell's log lines, or sees another's records, beside another.
static LOCK: Mutex<()> = Mutex::new(());
static INIT: Once = Once::new();
static WARNINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());
// The cards that say they are empty.
static EMPTY: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso_shell")
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) && record.level() <= log::Level::Warn {
            WARNINGS
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(format!("{} {}", record.level(), record.args()));
        }
    }
    fn flush(&self) {}
}

fn warnings() -> Vec<String> {
    WARNINGS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

fn set_empty(node: &str, empty: bool) {
    let mut set = EMPTY.lock().unwrap_or_else(|error| error.into_inner());
    if empty {
        set.insert(node.to_string());
    } else {
        set.remove(node);
    }
}

#[derive(Default)]
struct Captured {
    spans: Vec<String>,
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
        let mut captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        captured
            .spans
            .push(attributes.metadata().name().to_string());
        tracing::span::Id::from_u64(captured.spans.len() as u64)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("riso_shell") {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let line = format!("{}{}", event.metadata().level(), fields.0).replace('"', "");
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .lines
            .push(line);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

impl Trace {
    fn lines(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .lines
            .clone()
    }
    // One `state-save` group is one write of the file.
    fn state_saves(&self) -> usize {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .spans
            .iter()
            .filter(|name| *name == "state-save")
            .count()
    }
}

fn captured(cx: &mut TestAppContext, test: impl FnOnce(&mut TestAppContext, &Trace)) {
    INIT.call_once(|| {
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
    let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
    WARNINGS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clear();
    EMPTY
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clear();
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || test(cx, &trace));
}

struct Card {
    node: String,
}

impl shell::Emptiable for Card {
    fn is_empty(&self, _: &gpui::App) -> bool {
        EMPTY
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .contains(&self.node)
    }
}

// The names an item tabs container shows one tab for each.
struct Names(Vec<SharedString>);

impl shell::Items for Names {
    fn items(&self) -> Vec<SharedString> {
        self.0.clone()
    }
}

impl shell::ItemTabs for Names {
    fn close_item(&mut self, _: &SharedString, _: &mut gpui::Window, _: &mut gpui::Context<Self>) {}
    fn new_item(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) {}
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .debug_selector(|| format!("CARD-{}", self.node))
            .size_full()
    }
}

// `tour.card` and a row of `a` and the `right` container, of `container`,
// over `b`, `c`, `d`; the state file is set.
fn extension(app: &mut TestAppContext, container: &str, file: &Path) -> shell::Extension {
    app.update(|cx| {
        riso_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        host.register(cx, "tour.card", &[], |node, _, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
            })
            .into()
        });
        host.register_empty::<Card>(cx, "tour.card");
        host.place_root(cx, "root", "riso.row");
        host.place(cx, "a", "tour.card", "root", "children");
        host.place(cx, "right", container, "root", "children");
        for node in ["b", "c", "d"] {
            host.place(cx, node, "tour.card", "right", "children");
        }
        shell::set_state_file(cx, file.to_path_buf());
        host
    })
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
    app.simulate_window_resize(window.into(), gpui::size(px(900.), px(600.)));
    draw(window, app);
    window
}

fn settle(app: &mut TestAppContext) {
    app.run_until_parked();
    app.executor().advance_clock(Duration::from_millis(400));
    app.run_until_parked();
}

fn click(visual: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} has bounds"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
}

fn id(value: &str) -> NodeId {
    NodeId::parse(value).unwrap()
}

fn saved_view_state(file: &Path) -> serde_json::Value {
    let text = fs::read_to_string(file).unwrap();
    serde_json::from_str::<serde_json::Value>(&text).unwrap()["windows"][0].clone()
}

fn write_state(file: &Path, window: serde_json::Value) {
    fs::write(file, json!({"version": 1, "windows": [window]}).to_string()).unwrap();
}

// A named layout switch clears the given zoom, and the tabs containers it
// creates again start from their defaults: the saved tab is given once.
#[gpui::test]
fn a_named_layout_switch_clears_the_zoom_and_tabs_start_from_defaults(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("state.json");
        write_state(
            &file,
            json!({
                "right": {"component": "riso.tabs", "state": {"selected": "d"}},
                "window": {"component": "riso.window", "state": {"zoomed": "a"}},
            }),
        );
        let mut app = cx.new_app();
        let host = extension(&mut app, "riso.tabs", &file);
        // `tour.other` has no `right`: switching back creates it again.
        app.update(|cx| host.named_layout(cx, "tour.other", |layout| layout.remove("right")));
        let window = open_and_draw(&mut app);
        assert_eq!(
            app.update(|cx| shell::selected_tab(cx, "right")),
            Some(id("d"))
        );
        assert_eq!(app.update(|cx| shell::zoomed(cx)).as_deref(), Some("a"));

        window
            .update(&mut app, |_, window, cx| {
                host.show_layout(window, cx, "tour.other")
            })
            .unwrap();
        draw(window, &mut app);
        assert_eq!(app.update(|cx| shell::zoomed(cx)), None);
        assert_eq!(app.update(|cx| shell::selected_tab(cx, "right")), None);
        window
            .update(&mut app, |_, window, cx| {
                host.show_layout(window, cx, "riso.default")
            })
            .unwrap();
        draw(window, &mut app);
        assert_eq!(app.update(|cx| shell::zoomed(cx)), None);
        assert_eq!(
            app.update(|cx| shell::selected_tab(cx, "right")),
            Some(id("b")),
            "the recreated tabs container did not start from its default"
        );
        let given = trace
            .lines()
            .iter()
            .filter(|line| line.contains("tab given"))
            .count();
        assert_eq!(given, 1, "{:#?}", trace.lines());
        app.quit();
        app.run_until_parked();
        let saved = saved_view_state(&file);
        assert_eq!(saved["right"]["state"], json!({"selected": "b"}), "{saved}");
        assert_eq!(saved["window"]["state"], json!({"zoomed": null}), "{saved}");
    });
}

// A tabs container removed during the run drops its saved selection: the
// next save has no entry for it.
#[gpui::test]
fn a_removed_tabs_container_drops_its_saved_selection(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("state.json");
        let mut app = cx.new_app();
        let host = extension(&mut app, "riso.tabs", &file);
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        click(&mut visual, "TAB-d");
        draw(window, &mut app);
        settle(&mut app);
        assert_eq!(
            saved_view_state(&file)["right"]["state"],
            json!({"selected": "d"})
        );

        app.update(|cx| host.remove(cx, "right"));
        draw(window, &mut app);
        app.quit();
        app.run_until_parked();
        let saved = saved_view_state(&file);
        assert!(saved.get("right").is_none(), "{saved}");
        assert_eq!(saved["window"]["component"], "riso.window", "{saved}");
    });
}

// A saved `riso.tabs` state given to a node that is now of another
// component is skipped, with feature 1's `state skipped`.
#[gpui::test]
fn a_saved_tab_for_a_node_of_another_component_is_skipped(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("state.json");
        write_state(
            &file,
            json!({"right": {"component": "riso.tabs", "state": {"selected": "d"}}}),
        );
        let mut app = cx.new_app();
        let _host = extension(&mut app, "riso.column", &file);
        let _window = open_and_draw(&mut app);
        let lines = trace.lines();
        assert!(
            lines.iter().any(|line| {
                line.starts_with("DEBUG")
                    && line.contains("state skipped")
                    && line.contains("node=right")
                    && line.contains("saved=riso.tabs")
                    && line.contains("component=riso.column")
            }),
            "{lines:#?}"
        );
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("tab given") || line.contains("saved tab has no tab")),
            "{lines:#?}"
        );
        assert!(
            !lines
                .iter()
                .any(|line| line.starts_with("WARN") || line.starts_with("ERROR")),
            "{lines:#?}"
        );
    });
}

// Clicking the tab that is already selected changes nothing, and saves
// nothing.
#[gpui::test]
fn clicking_the_selected_tab_does_not_save(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("state.json");
        let mut app = cx.new_app();
        let _host = extension(&mut app, "riso.tabs", &file);
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        settle(&mut app);
        let saves = trace.state_saves();

        click(&mut visual, "TAB-b");
        draw(window, &mut app);
        settle(&mut app);
        assert_eq!(
            app.update(|cx| shell::selected_tab(cx, "right")),
            Some(id("b"))
        );
        assert_eq!(trace.state_saves(), saves, "the selected tab's click saved");

        click(&mut visual, "TAB-c");
        draw(window, &mut app);
        settle(&mut app);
        assert_eq!(
            trace.state_saves(),
            saves + 1,
            "a new selection did not save"
        );
        app.quit();
        app.run_until_parked();
    });
}

// An item tab's selection is saved as its copy's ID, `template@key`, and
// given back at the next run, once the copies exist, without a warning.
#[gpui::test]
fn a_selected_item_tab_is_selected_again_at_the_next_run(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("state.json");
        let items = |app: &mut TestAppContext| {
            app.update(|cx| {
                riso_ui::init(cx);
                let host = shell::Extension::new(cx, "tour").unwrap();
                let names = cx.new(|_| Names(vec!["ada".into(), "bob".into(), "cy".into()]));
                host.provide_window(cx, names);
                host.register(cx, "tour.card", &[], |node, _, _, cx| {
                    cx.new(|_| Card {
                        node: node.to_string(),
                    })
                    .into()
                });
                host.place_root(cx, "root", "riso.row");
                host.place(cx, "tabs", "riso.tabs", "root", "children");
                host.item_tabs::<Names>(cx, "tabs");
                host.place(cx, "name", "tour.card", "tabs", "children");
                shell::set_state_file(cx, file.clone());
            })
        };
        let selected = |app: &mut TestAppContext| {
            app.update(|cx| shell::selected_tab(cx, "tabs").map(|node| node.to_string()))
        };

        let mut app = cx.new_app();
        items(&mut app);
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        click(&mut visual, "TAB-name@bob");
        draw(window, &mut app);
        assert_eq!(selected(&mut app).as_deref(), Some("name@bob"));
        app.quit();
        app.run_until_parked();
        assert_eq!(
            saved_view_state(&file)["tabs"]["state"],
            json!({"selected": "name@bob"})
        );

        let mut restart = cx.new_app();
        items(&mut restart);
        let _window = open_and_draw(&mut restart);
        assert_eq!(selected(&mut restart).as_deref(), Some("name@bob"));
        assert!(
            trace
                .lines()
                .iter()
                .any(|line| line.contains("tab given") && line.contains("tab=name@bob")),
            "{:#?}",
            trace.lines()
        );
        assert!(warnings().is_empty(), "{:?}", warnings());
    });
}

// A saved tab that has no tab when the window's views are created waits
// for its container's first layout, here held off by a zoom: until then
// `selected_tab` gives what is selected and a save keeps the saved tab; the
// layout then gives it, once.
#[gpui::test]
fn a_saved_tab_with_no_tab_at_start_waits_for_the_first_layout(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("state.json");
        write_state(
            &file,
            json!({
                "right": {"component": "riso.tabs", "state": {"selected": "d"}},
                "window": {"component": "riso.window", "state": {"zoomed": "a"}},
            }),
        );
        set_empty("d", true);
        let mut app = cx.new_app();
        let host = extension(&mut app, "riso.tabs", &file);
        let window = open_and_draw(&mut app);
        assert_eq!(app.update(|cx| shell::zoomed(cx)).as_deref(), Some("a"));
        assert_eq!(
            app.update(|cx| shell::selected_tab(cx, "right")),
            Some(id("b")),
            "the waiting tab is not what is selected"
        );
        app.update(shell::state_changed);
        settle(&mut app);
        assert_eq!(
            saved_view_state(&file)["right"]["state"],
            json!({"selected": "d"}),
            "a save while the tab waits must keep it"
        );

        set_empty("d", false);
        app.update(|cx| host.zoom_out(cx));
        draw(window, &mut app);
        draw(window, &mut app);
        assert_eq!(
            app.update(|cx| shell::selected_tab(cx, "right")),
            Some(id("d"))
        );
        let lines = trace.lines();
        let decisions = lines
            .iter()
            .filter(|line| line.contains("tab given") || line.contains("saved tab has no tab"))
            .collect::<Vec<_>>();
        assert_eq!(decisions.len(), 1, "{lines:#?}");
        assert!(decisions[0].contains("tab given"), "{lines:#?}");
        assert!(warnings().is_empty(), "{:?}", warnings());
    });
}

// A named layout switch that clears a zoom saves it after the wait, so the
// file does not keep the old zoom.
#[gpui::test]
fn a_layout_switch_that_clears_the_zoom_saves(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("state.json");
        let mut app = cx.new_app();
        let host = extension(&mut app, "riso.tabs", &file);
        app.update(|cx| host.named_layout(cx, "tour.other", |_| ()));
        let window = open_and_draw(&mut app);
        app.update(|cx| host.set_zoomed(cx, "a"));
        settle(&mut app);
        assert_eq!(
            saved_view_state(&file)["window"]["state"],
            json!({"zoomed": "a"})
        );
        let saves = trace.state_saves();
        window
            .update(&mut app, |_, window, cx| {
                host.show_layout(window, cx, "tour.other")
            })
            .unwrap();
        settle(&mut app);
        assert_eq!(trace.state_saves(), saves + 1);
        assert_eq!(
            saved_view_state(&file)["window"]["state"],
            json!({"zoomed": null})
        );
    });
}

// Zooming the node already zoomed changes nothing, and saves nothing.
#[gpui::test]
fn zooming_the_zoomed_node_again_does_not_save(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("state.json");
        let mut app = cx.new_app();
        let host = extension(&mut app, "riso.tabs", &file);
        let _window = open_and_draw(&mut app);
        app.update(|cx| host.set_zoomed(cx, "a"));
        settle(&mut app);
        let saves = trace.state_saves();
        app.update(|cx| host.set_zoomed(cx, "a"));
        settle(&mut app);
        assert_eq!(trace.state_saves(), saves, "zooming the zoomed node saved");
        app.update(|cx| host.set_zoomed(cx, "b"));
        settle(&mut app);
        assert_eq!(trace.state_saves(), saves + 1, "a new zoom did not save");
    });
}
