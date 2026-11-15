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

// Hints of a `min` of `width` by `height` rems, and a `preferred` width of
// `preferred` rems where one is given.
fn hints(width: f32, height: f32, preferred: Option<f32>) -> shell::SizeHints {
    shell::SizeHints {
        min: gpui::size(gpui::rems(width), gpui::rems(height)),
        preferred: gpui::size(preferred.map(gpui::rems), None),
    }
}

fn widths(visual: &mut gpui::VisualTestContext, selectors: &[&'static str]) -> Vec<gpui::Pixels> {
    selectors
        .iter()
        .map(|selector| {
            visual
                .debug_bounds(selector)
                .expect("element must be present")
                .size
                .width
        })
        .collect()
}

// The point where a child's right edge is, which is where the handle of the
// split that follows it sits.
fn handle(
    visual: &mut gpui::VisualTestContext,
    selector: &'static str,
) -> gpui::Point<gpui::Pixels> {
    let bounds = visual
        .debug_bounds(selector)
        .expect("element must be present");
    gpui::point(bounds.right(), bounds.center().y)
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

// Hints declared after the shell exists still give a child with no entry its
// preferred size.
#[gpui::test]
fn hints_declared_after_the_shell_exists_apply(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let settings = temp.path().join("settings.json");
    let mut app = cx.new_app();
    let host = extension_with_settings(&mut app, &settings);
    register_cards(&mut app, &host);
    app.update(|cx| {
        host.place_root(cx, "root", "riso.row");
        host.place(cx, "panel", "tour.panel", "root", "children");
        host.place(cx, "main", "tour.card", "root", "children");
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    assert_eq!(
        widths(&mut visual, &["CARD-panel", "CARD-main"]),
        [px(450.), px(450.)]
    );
    app.update(|cx| host.size_hints(cx, "tour.panel", hints(5., 6.25, Some(15.))));
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-panel", "CARD-main"]),
        [px(240.), px(660.)]
    );
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
}

// A drag that ends where it started leaves the preferred size a preferred
// size: nothing is recorded, and it still follows the UI font.
#[gpui::test]
fn a_drag_back_to_the_start_keeps_the_preferred_size(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let settings = temp.path().join("settings.json");
    let mut app = cx.new_app();
    let host = extension_with_settings(&mut app, &settings);
    register_cards(&mut app, &host);
    app.update(|cx| {
        host.size_hints(cx, "tour.panel", hints(5., 6.25, Some(15.)));
        host.place_root(cx, "root", "riso.row");
        host.place(cx, "panel", "tour.panel", "root", "children");
        host.place(cx, "main", "tour.card", "root", "children");
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    assert_eq!(
        widths(&mut visual, &["CARD-panel", "CARD-main"]),
        [px(240.), px(660.)]
    );
    let at = handle(&mut visual, "CARD-panel");
    let away = gpui::point(px(400.), at.y);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(away, gpui::MouseButton::Left, Default::default());
    // Frames during the drag show the dragged size.
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-panel", "CARD-main"]),
        [px(400.), px(500.)]
    );
    visual.simulate_mouse_move(at, gpui::MouseButton::Left, Default::default());
    draw(window, &mut app);
    visual.simulate_mouse_up(at, gpui::MouseButton::Left, Default::default());
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-panel", "CARD-main"]),
        [px(240.), px(660.)]
    );
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    // Still a preferred size: 15 rems of 20 px.
    set_font_size(&mut app, &settings, 20.);
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-panel", "CARD-main"]),
        [px(300.), px(600.)]
    );
}
