use gpui::{AppContext, IntoElement, Styled, TestAppContext};
use yard_shell::{self as shell, FocusRight};

struct Card;
impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full()
    }
}

fn draw(window: gpui::WindowHandle<shell::Shell>, cx: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn focus(visual: &mut gpui::VisualTestContext, node: &str) {
    visual.update(|window, cx| shell::focus_handle(cx, node).unwrap().focus(window, cx));
}

fn focused(visual: &mut gpui::VisualTestContext) -> Option<String> {
    visual.update(|window, cx| shell::focused_node(window, cx).map(|node| node.to_string()))
}

fn setup(
    cx: &mut TestAppContext,
    place: impl FnOnce(&mut gpui::App, &shell::Extension),
) -> (shell::Extension, gpui::WindowHandle<shell::Shell>) {
    let ext = cx.update(|cx| {
        yard_ui::init(cx);
        let ext = shell::Extension::new(cx, "tour").unwrap();
        ext.register(cx, "tour.card", &[], |_, _, _, cx| cx.new(|_| Card).into());
        ext.place_root(cx, "root", "yard.row");
        ext.place(cx, "a", "tour.card", "root", "children");
        place(cx, &ext);
        ext
    });
    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(640.), gpui::px(480.)));
    draw(window, cx);
    (ext, window)
}

#[gpui::test]
fn empty_builtin_containers_are_never_direction_targets(cx: &mut TestAppContext) {
    for component in ["yard.row", "yard.column", "yard.tabs"] {
        // Each layout needs its own application registry.
        let mut app = cx.new_app();
        let (_, window) = setup(&mut app, |cx, ext| {
            ext.place(cx, "empty", component, "root", "children");
        });
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        focus(&mut visual, "a");
        visual.dispatch_action(FocusRight);
        draw(window, &mut app);
        assert_eq!(focused(&mut visual).as_deref(), Some("a"), "{component}");
    }
}

#[gpui::test]
fn hidden_and_removed_nodes_with_old_bounds_are_not_targets(cx: &mut TestAppContext) {
    let (ext, window) = setup(cx, |cx, ext| {
        ext.place(cx, "right", "yard.column", "root", "children");
        ext.place(cx, "b", "tour.card", "right", "children");
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    focus(&mut visual, "a");
    visual.update(|_, cx| ext.set_hidden(cx, "b", true));
    // Before a redraw, b's old rectangle still contains the probe.
    visual.dispatch_action(FocusRight);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    visual.update(|_, cx| ext.set_hidden(cx, "b", false));
    draw(window, cx);
    visual.update(|_, cx| ext.remove(cx, "b"));
    visual.dispatch_action(FocusRight);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
}

#[gpui::test]
fn zoom_uses_only_the_zoomed_subtree_even_with_old_neighbor_bounds(cx: &mut TestAppContext) {
    let (ext, window) = setup(cx, |cx, ext| {
        ext.place(cx, "b", "tour.card", "root", "children");
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    focus(&mut visual, "a");
    visual.update(|_, cx| ext.set_zoomed(cx, "a"));
    visual.dispatch_action(FocusRight);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    draw(window, cx);
    visual.dispatch_action(FocusRight);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    visual.update(|_, cx| ext.zoom_out(cx));
    draw(window, cx);
    visual.dispatch_action(FocusRight);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
}

#[gpui::test]
fn reset_drops_runtime_nodes_and_direction_finds_the_restored_leaf(cx: &mut TestAppContext) {
    let (ext, window) = setup(cx, |cx, ext| {
        ext.place(cx, "right", "yard.column", "root", "children");
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| ext.add(window, cx, "b", "tour.card", "right", "children"));
    draw(window, cx);
    focus(&mut visual, "a");
    visual.dispatch_action(FocusRight);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
    visual.update(|window, cx| ext.reset_layout(window, cx));
    draw(window, cx);
    focus(&mut visual, "a");
    visual.dispatch_action(FocusRight);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    visual.update(|window, cx| ext.add(window, cx, "b", "tour.card", "right", "children"));
    draw(window, cx);
    visual.dispatch_action(FocusRight);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
}
