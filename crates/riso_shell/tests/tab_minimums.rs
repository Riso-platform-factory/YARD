use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use riso_shell as shell;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "riso-shell-component-sizes-{}-{}-{nanos:x}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// A card fills the zone its split gives it, found as `CARD-<node>`.
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
    window.update(app, |_, window, _| window.refresh()).unwrap();
    draw(window, app);
    window
}

// The components of these tests: `card` has no hints, `wide` a wide minimum,
// `panel` a preferred width.
fn register_cards(app: &mut TestAppContext, host: &shell::Extension) {
    app.update(|cx| {
        for component in ["tour.card", "tour.wide", "tour.panel"] {
            host.register(cx, component, &[], |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                })
                .into()
            });
        }
    });
}

// An extension `tour` over a settings file whose UI font size is 16.
fn extension_with_settings(
    app: &mut TestAppContext,
    settings: &std::path::Path,
) -> shell::Extension {
    fs::write(settings, r#"{"theme": {"ui_font_size": 16}}"#).unwrap();
    let settings = settings.to_path_buf();
    app.update(|cx| {
        riso_settings::init(cx);
        riso_settings::load(cx, settings);
        riso_settings::watch(cx);
        riso_ui::init(cx);
        riso_ui::apply_theme_setting(cx);
        shell::Extension::new(cx, "tour").unwrap()
    })
}

fn set_font_size(app: &mut TestAppContext, settings: &std::path::Path, size: f32) {
    fs::write(
        settings,
        format!(r#"{{"theme": {{"ui_font_size": {size}}}}}"#),
    )
    .unwrap();
    app.executor().advance_clock(Duration::from_secs(1));
    app.run_until_parked();
    app.read(|cx| assert_eq!(riso_ui::ui_font_size(cx), px(size)));
}

fn heights(visual: &mut gpui::VisualTestContext, selector: &'static str) -> gpui::Pixels {
    visual
        .debug_bounds(selector)
        .expect("element must be present")
        .size
        .height
}

// A tabs container's minimum is the largest minimum of its children plus the
// height of its tab bar, right on the first frame and after a font change. A
// fixed sibling larger than the column shrinks the flexible tabs container to
// exactly its minimum.
#[gpui::test]
fn a_tabs_containers_minimum_counts_its_tab_bar_from_the_first_frame(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let settings = temp.path().join("settings.json");
    let mut app = cx.new_app();
    let host = extension_with_settings(&mut app, &settings);
    register_cards(&mut app, &host);
    app.update(|cx| {
        host.place_root_with_props(
            cx,
            "root",
            "riso.column",
            serde_json::json!({"sizes": {"plain": 2000}}),
        );
        host.place(cx, "tabs", "riso.tabs", "root", "children");
        host.place(cx, "a", "tour.card", "tabs", "children");
        host.place(cx, "plain", "tour.card", "root", "children");
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    // The tab's content keeps the card's default 6.25 rems: 100 px.
    assert_eq!(heights(&mut visual, "CARD-a"), px(100.));
    set_font_size(&mut app, &settings, 20.);
    draw(window, &mut app);
    // 6.25 rems of 20 px, with the tab bar on top.
    assert_eq!(heights(&mut visual, "CARD-a"), px(125.));
}
