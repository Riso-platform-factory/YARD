#![cfg(feature = "dev")]

use gpui::{
    AnyView, AppContext, Bounds, FocusHandle, InteractiveElement, KeyBinding, Modifiers,
    MouseButton, ParentElement, Pixels, Point, StatefulInteractiveElement, Styled, TestAppContext,
    VisualTestContext, div, point, px, size,
};
use riso_ui::{Button, IconButton, IconName, Label, dev::Tooltip, v_flex};
use std::time::Duration;

gpui::actions!(tooltip_test, [Quit, Unbound]);

#[cfg(target_os = "macos")]
const QUIT_KEYS: &str = "cmd-q";
#[cfg(not(target_os = "macos"))]
const QUIT_KEYS: &str = "ctrl-q";

// Comfortably past GPUI's 500 ms show delay.
const SHOWN_AFTER: Duration = Duration::from_millis(600);

// The selector the marked wrapper paints under.
const TIP: &str = "TOOLTIP";

// A window of a known size: 400px wide, 600px tall.
fn open_test_window(
    view: impl gpui::Render + 'static,
    cx: &mut TestAppContext,
) -> VisualTestContext {
    let window = cx.open_window(size(px(400.), px(600.)), |_, _| view);
    VisualTestContext::from_window(window.into(), cx)
}

// Draws a frame so `debug_bounds` sees the painted elements. A shown tooltip
// prepaints inside `window.draw`, so this also captures its bounds.
fn draw_a_frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

// The center of the element a selector marks, from a fresh frame.
fn center_of(cx: &mut VisualTestContext, selector: &'static str) -> Point<Pixels> {
    draw_a_frame(cx);
    cx.debug_bounds(selector)
        .expect("the target draws")
        .center()
}

// Rests the pointer at `position`, advances past the 500 ms show delay, draws
// a frame, and returns the shown tooltip's bounds.
fn shown_tip_at(cx: &mut VisualTestContext, position: Point<Pixels>) -> Bounds<Pixels> {
    cx.simulate_mouse_move(position, None, Modifiers::default());
    cx.executor().advance_clock(SHOWN_AFTER);
    cx.run_until_parked();
    draw_a_frame(cx);
    cx.debug_bounds(TIP).expect("a tooltip shows")
}

// The shown tooltip's bounds over the element a selector marks.
fn shown_tip_over(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    let position = center_of(cx, selector);
    shown_tip_at(cx, position)
}

// A view that marks where a built tooltip paints: the contract's builders give
// an opaque `AnyView`, so the tests wrap it in a `debug_selector` of their own.
struct Marked {
    view: AnyView,
    selector: &'static str,
}

impl gpui::Render for Marked {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        let selector = self.selector;
        div()
            .debug_selector(move || selector.into())
            .child(self.view.clone())
    }
}

// Wraps a tooltip builder so the shown tooltip lands in `debug_bounds`.
fn marked(
    selector: &'static str,
    build: impl Fn(&mut gpui::Window, &mut gpui::App) -> AnyView + 'static,
) -> impl Fn(&mut gpui::Window, &mut gpui::App) -> AnyView {
    move |window, cx| {
        let view = build(window, cx);
        cx.new(|_| Marked { view, selector }).into()
    }
}

struct Titled;

impl gpui::Render for Titled {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex()
            .size_full()
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .debug_selector(|| "SAVE".into())
                    .child(Button::new("save", "Save").tooltip(marked(TIP, Tooltip::text("Save")))),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .debug_selector(|| "BLANK".into())
                    .child(Button::new("blank", "Blank").tooltip(marked(TIP, Tooltip::text("")))),
            )
    }
}

#[gpui::test]
fn a_button_shows_its_text_tooltip_only_after_the_hover_delay_and_hides_it_as_the_pointer_leaves(
    cx: &mut TestAppContext,
) {
    cx.update(riso_ui::init);
    let mut cx = open_test_window(Titled, cx);
    let rest = center_of(&mut cx, "SAVE");

    cx.simulate_mouse_move(rest, None, Modifiers::default());
    draw_a_frame(&mut cx);
    assert!(
        cx.debug_bounds(TIP).is_none(),
        "the pointer just arrived: no tooltip yet"
    );

    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    draw_a_frame(&mut cx);
    assert!(
        cx.debug_bounds(TIP).is_none(),
        "400 ms in, short of the 500 ms delay: no tooltip yet"
    );

    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    draw_a_frame(&mut cx);
    let tip = cx
        .debug_bounds(TIP)
        .expect("the delay passed: the tooltip shows");
    assert!(
        tip.size.width > px(0.) && tip.size.height > px(0.),
        "the tooltip is a real box: {tip:?}"
    );
    // GPUI draws the tooltip a pixel below and right of where the pointer rests.
    assert!(
        (f32::from(tip.origin.x - rest.x) - 1.).abs() <= 0.5
            && (f32::from(tip.origin.y - rest.y) - 1.).abs() <= 0.5,
        "the tooltip sits a pixel below and right of the pointer: {tip:?} vs {rest:?}"
    );

    let blank_tip = shown_tip_over(&mut cx, "BLANK");
    assert!(
        tip.size.width > blank_tip.size.width,
        "the shown box carries the title's text, wider than an empty title's: {tip:?} vs {blank_tip:?}"
    );

    cx.simulate_mouse_move(point(px(6.), px(6.)), None, Modifiers::default());
    draw_a_frame(&mut cx);
    assert!(
        cx.debug_bounds(TIP).is_none(),
        "the pointer left: no tooltip shows"
    );
}

#[gpui::test]
fn pressing_the_mouse_on_the_button_hides_its_shown_tooltip(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let mut cx = open_test_window(Titled, cx);
    let rest = center_of(&mut cx, "SAVE");
    shown_tip_at(&mut cx, rest);

    cx.simulate_mouse_down(rest, MouseButton::Left, Modifiers::default());
    draw_a_frame(&mut cx);
    assert!(
        cx.debug_bounds(TIP).is_none(),
        "a press on the button hid its tooltip"
    );
    cx.simulate_mouse_up(rest, MouseButton::Left, Modifiers::default());
}

struct Kinds;

impl gpui::Render for Kinds {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex()
            .size_full()
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .debug_selector(|| "ICON".into())
                    .child(
                        IconButton::new("check", IconName::Check)
                            .tooltip(marked(TIP, Tooltip::text("Check it"))),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .debug_selector(|| "PLAIN".into())
                    .child(
                        div()
                            .id("plain-target")
                            .tooltip(marked(TIP, Tooltip::text("Plain tip")))
                            .child(Label::new("Plain")),
                    ),
            )
    }
}

#[gpui::test]
fn an_icon_button_and_a_plain_element_with_an_id_take_a_tooltip_the_same_way(
    cx: &mut TestAppContext,
) {
    cx.update(riso_ui::init);
    let mut cx = open_test_window(Kinds, cx);

    let icon_tip = shown_tip_over(&mut cx, "ICON");
    assert!(
        icon_tip.size.width > px(0.) && icon_tip.size.height > px(0.),
        "the icon button's tooltip shows: {icon_tip:?}"
    );

    let plain_tip = shown_tip_over(&mut cx, "PLAIN");
    assert!(
        plain_tip.size.width > px(0.) && plain_tip.size.height > px(0.),
        "the plain element's tooltip shows: {plain_tip:?}"
    );
}

struct Disabled;

impl gpui::Render for Disabled {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex().size_full().child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .debug_selector(|| "OFF".into())
                .child(
                    Button::new("off", "Off")
                        .disabled(true)
                        .tooltip(marked(TIP, Tooltip::text("Why it is off"))),
                ),
        )
    }
}

#[gpui::test]
fn a_disabled_button_shows_its_tooltip(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let mut cx = open_test_window(Disabled, cx);
    let tip = shown_tip_over(&mut cx, "OFF");
    assert!(
        tip.size.width > px(0.) && tip.size.height > px(0.),
        "the disabled button's tooltip shows: {tip:?}"
    );
}

struct Keyed {
    focus: FocusHandle,
}

impl gpui::Render for Keyed {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex()
            .size_full()
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .debug_selector(|| "TEXT".into())
                    .child(
                        Button::new("plain", "Save").tooltip(marked(TIP, Tooltip::text("Save"))),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .debug_selector(|| "KEYS".into())
                    .child(
                        Button::new("bound", "Save")
                            .tooltip(marked(TIP, Tooltip::for_action("Save", &Quit))),
                    ),
            )
            .child(
                div()
                    .track_focus(&self.focus)
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .debug_selector(|| "IN".into())
                    .child(Button::new("focused", "Save").tooltip(marked(
                        TIP,
                        Tooltip::for_action_in("Save", &Quit, &self.focus),
                    ))),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .debug_selector(|| "FREE".into())
                    .child(
                        Button::new("unbound", "Save")
                            .tooltip(marked(TIP, Tooltip::for_action("Save", &Unbound))),
                    ),
            )
    }
}

#[gpui::test]
fn a_bound_actions_tooltip_is_wider_than_the_same_title_as_text_while_an_unbound_actions_matches(
    cx: &mut TestAppContext,
) {
    cx.update(riso_ui::init);
    cx.update(|cx| {
        cx.bind_keys([KeyBinding::new(QUIT_KEYS, Quit, None)]);
    });
    let focus = cx.update(|cx| cx.focus_handle());
    let mut cx = open_test_window(Keyed { focus }, cx);

    let text_width = shown_tip_over(&mut cx, "TEXT").size.width;
    let keys_tip = shown_tip_over(&mut cx, "KEYS");
    assert!(
        cx.debug_bounds("KEY_BINDING-q").is_some(),
        "the bound action's keys draw inside the tooltip"
    );
    let in_tip = shown_tip_over(&mut cx, "IN");
    let free_width = shown_tip_over(&mut cx, "FREE").size.width;

    assert!(
        keys_tip.size.width > text_width,
        "the keys take room at the title's right: {:?} vs {text_width:?}",
        keys_tip.size.width
    );
    assert!(
        in_tip.size.width > text_width,
        "the keys read from a given focus take the same room: {:?} vs {text_width:?}",
        in_tip.size.width
    );
    assert!(
        (f32::from(free_width) - f32::from(text_width)).abs() <= 0.5,
        "an unbound action takes no room, as wide as the title alone: {free_width:?} vs {text_width:?}"
    );
}

struct WithMeta;

impl gpui::Render for WithMeta {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex()
            .size_full()
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .debug_selector(|| "BARE".into())
                    .child(Button::new("bare", "Save").tooltip(marked(TIP, Tooltip::text("Save")))),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .debug_selector(|| "META".into())
                    .child(Button::new("detailed", "Save").tooltip(marked(
                        TIP,
                        Tooltip::with_meta("Save", None, "Kept a copy in the folder"),
                    ))),
            )
    }
}

#[gpui::test]
fn a_tooltip_with_a_detail_line_draws_taller_than_one_without(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let mut cx = open_test_window(WithMeta, cx);
    let bare_height = shown_tip_over(&mut cx, "BARE").size.height;
    let meta_tip = shown_tip_over(&mut cx, "META");
    assert!(
        meta_tip.size.height > bare_height,
        "the detail line adds height: {:?} vs {bare_height:?}",
        meta_tip.size.height
    );
}

struct Long;

impl gpui::Render for Long {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex()
            .size_full()
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .debug_selector(|| "SHORT".into())
                    .child(
                        Button::new("short", "Save").tooltip(marked(TIP, Tooltip::text("Save"))),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .debug_selector(|| "LONG".into())
                    .child(Button::new("long", "Save").tooltip(marked(
                        TIP,
                        Tooltip::text(
                            "Save the file to disk and keep a backup copy of the \
                             previous version in the same folder",
                        ),
                    ))),
            )
    }
}

#[gpui::test]
fn a_long_title_wraps_within_eighteen_rems(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let mut cx = open_test_window(Long, cx);
    let short_height = shown_tip_over(&mut cx, "SHORT").size.height;
    let long_tip = shown_tip_over(&mut cx, "LONG");
    // Zed caps the title at 18 rems: 288px at the default 16px rem. Around it
    // the box adds its own padding, border, and the pointer offset: ~26px.
    assert!(
        long_tip.size.width <= px(315.),
        "the box stays within 18 rems plus its padding: {long_tip:?}"
    );
    assert!(
        long_tip.size.height > short_height,
        "the long title wrapped to several lines: {:?} vs {short_height:?}",
        long_tip.size.height
    );
}
