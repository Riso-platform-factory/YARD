#![cfg(feature = "dev")]

use gpui::{
    FocusHandle, InteractiveElement, KeyBinding, ParentElement, Styled, TestAppContext,
    VisualTestContext, div, px, size,
};
use riso_ui::{
    dev::{KeyHint, key_hint_text},
    v_flex,
};

gpui::actions!(
    key_hint_test,
    [ToggleZoom, FocusLeft, Quit, Scoped, Unbound]
);

#[cfg(target_os = "macos")]
const FOCUS_LEFT_KEYS: &str = "cmd-k cmd-left";
#[cfg(not(target_os = "macos"))]
const FOCUS_LEFT_KEYS: &str = "ctrl-k ctrl-left";
#[cfg(target_os = "macos")]
const QUIT_KEYS: &str = "cmd-q";
#[cfg(not(target_os = "macos"))]
const QUIT_KEYS: &str = "ctrl-q";

// The shell's default-key shapes, bound here because riso_shell depends on
// riso_ui, so a dev-dependency back to it would make a cycle.
fn bind_test_keys(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.bind_keys([
            KeyBinding::new("shift-escape", ToggleZoom, Some("Shell")),
            KeyBinding::new(FOCUS_LEFT_KEYS, FocusLeft, Some("Shell")),
            KeyBinding::new(QUIT_KEYS, Quit, None),
            // The deeper-context binding is bound after the shallower one:
            // where both match, the last bound is the one that would run.
            KeyBinding::new("ctrl-1", Scoped, Some("Shell")),
            KeyBinding::new("ctrl-shift-alt-cmd-2", Scoped, Some("ModalLayer")),
        ]);
    });
}

// A window of a known size: 400px wide, 600px tall.
fn open_test_window(
    view: impl gpui::Render + 'static,
    cx: &mut TestAppContext,
) -> VisualTestContext {
    let window = cx.open_window(size(px(400.), px(600.)), |_, _| view);
    VisualTestContext::from_window(window.into(), cx)
}

// Draws a frame so `debug_bounds` sees the wrapped elements. A hint resolves
// its binding against the frame drawn before it, so bound hints need two.
fn draw_a_frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

struct ShellText;

impl gpui::Render for ShellText {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex().size_full().key_context("Shell")
    }
}

#[gpui::test]
fn key_hint_text_for_a_bound_action_gives_zeds_text_on_this_os(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    bind_test_keys(cx);
    let mut cx = open_test_window(ShellText, cx);
    draw_a_frame(&mut cx);
    let zoom = cx.update(|window, cx| key_hint_text(&ToggleZoom, window, cx));
    let quit = cx.update(|window, cx| key_hint_text(&Quit, window, cx));
    #[cfg(target_os = "macos")]
    let (zoom_expected, quit_expected) = ("Shift-Escape", "Command-Q");
    #[cfg(not(target_os = "macos"))]
    let (zoom_expected, quit_expected) = ("Shift-Escape", "Ctrl-Q");
    assert_eq!(
        zoom.as_deref(),
        Some(zoom_expected),
        "shift-escape, as Zed's text_for_keystroke writes it on this OS"
    );
    assert_eq!(
        quit.as_deref(),
        Some(quit_expected),
        "a binding bound without a context"
    );
}

#[gpui::test]
fn key_hint_text_for_a_two_keystroke_binding_holds_both_keystrokes_in_order(
    cx: &mut TestAppContext,
) {
    cx.update(riso_ui::init);
    bind_test_keys(cx);
    let mut cx = open_test_window(ShellText, cx);
    draw_a_frame(&mut cx);
    let left = cx.update(|window, cx| key_hint_text(&FocusLeft, window, cx));
    #[cfg(target_os = "macos")]
    let expected = "Command-K Command-Left";
    #[cfg(not(target_os = "macos"))]
    let expected = "Ctrl-K Ctrl-Left";
    assert_eq!(
        left.as_deref(),
        Some(expected),
        "the chord's two keystrokes, in order"
    );
}

struct UnboundHint;

impl gpui::Render for UnboundHint {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex()
            .size_full()
            .items_start()
            .key_context("Shell")
            .child(
                div()
                    .debug_selector(|| "UNBOUND_HINT".into())
                    .child(KeyHint::for_action(&Unbound, cx)),
            )
    }
}

#[gpui::test]
fn an_action_with_no_binding_gives_no_text_and_its_hint_takes_no_room(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    bind_test_keys(cx);
    let mut cx = open_test_window(UnboundHint, cx);
    draw_a_frame(&mut cx);
    let text = cx.update(|window, cx| key_hint_text(&Unbound, window, cx));
    draw_a_frame(&mut cx);
    assert!(text.is_none(), "no binding, no text: {text:?}");
    let bounds = cx
        .debug_bounds("UNBOUND_HINT")
        .expect("the wrapper around the hint draws");
    assert_eq!(
        bounds.size,
        size(px(0.), px(0.)),
        "the hint draws nothing and takes no room"
    );
}

struct ScopedHints {
    outer: FocusHandle,
    inner: FocusHandle,
}

impl gpui::Render for ScopedHints {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex()
            .size_full()
            .items_start()
            .key_context("Shell")
            .child(
                div()
                    .track_focus(&self.outer)
                    .debug_selector(|| "OUTER_HINT".into())
                    .child(KeyHint::for_action_in(&Scoped, &self.outer, cx)),
            )
            .child(
                div().key_context("ModalLayer").child(
                    div()
                        .track_focus(&self.inner)
                        .debug_selector(|| "INNER_HINT".into())
                        .child(KeyHint::for_action_in(&Scoped, &self.inner, cx)),
                ),
            )
    }
}

#[gpui::test]
fn for_action_in_from_a_focus_inside_the_deeper_context_shows_the_deeper_binding(
    cx: &mut TestAppContext,
) {
    cx.update(riso_ui::init);
    bind_test_keys(cx);
    let outer = cx.update(|cx| cx.focus_handle());
    let inner = cx.update(|cx| cx.focus_handle());
    let mut cx = open_test_window(ScopedHints { outer, inner }, cx);
    draw_a_frame(&mut cx);
    draw_a_frame(&mut cx);
    let outer_bounds = cx
        .debug_bounds("OUTER_HINT")
        .expect("the hint from the shallower context draws");
    let inner_bounds = cx
        .debug_bounds("INNER_HINT")
        .expect("the hint from the deeper context draws");
    assert!(
        outer_bounds.size.width > px(0.),
        "from the shallower context the shallower binding shows: {outer_bounds:?}"
    );
    assert!(
        inner_bounds.size.width > outer_bounds.size.width,
        "from inside the deeper context the deeper binding shows, \
        five key caps over two: {inner_bounds:?} vs {outer_bounds:?}"
    );
}

struct BoundHints {
    focus: FocusHandle,
}

impl gpui::Render for BoundHints {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex()
            .size_full()
            .items_start()
            .key_context("Shell")
            .child(
                div()
                    .track_focus(&self.focus)
                    .debug_selector(|| "ZOOM_HINT".into())
                    .child(KeyHint::for_action(&ToggleZoom, cx)),
            )
            .child(
                div()
                    .debug_selector(|| "LEFT_HINT".into())
                    .child(KeyHint::for_action(&FocusLeft, cx)),
            )
    }
}

#[gpui::test]
fn a_hint_for_a_bound_action_takes_room_and_its_width_grows_with_the_number_of_keys(
    cx: &mut TestAppContext,
) {
    cx.update(riso_ui::init);
    bind_test_keys(cx);
    let focus = cx.update(|cx| cx.focus_handle());
    let mut cx = open_test_window(
        BoundHints {
            focus: focus.clone(),
        },
        cx,
    );
    cx.update(|window, cx| focus.focus(window, cx));
    draw_a_frame(&mut cx);
    draw_a_frame(&mut cx);
    let zoom = cx
        .debug_bounds("ZOOM_HINT")
        .expect("the one-keystroke hint draws");
    let left = cx
        .debug_bounds("LEFT_HINT")
        .expect("the two-keystroke hint draws");
    assert!(
        zoom.size.width > px(0.) && zoom.size.height > px(0.),
        "a bound action's hint takes room: {zoom:?}"
    );
    assert!(
        left.size.width > zoom.size.width,
        "two keystrokes draw wider than one: {left:?} vs {zoom:?}"
    );
}
