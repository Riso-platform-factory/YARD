use gpui::{
    IntoElement, ParentElement, ScrollDelta, ScrollEvent, ScrollHandle, Styled, TestAppContext,
    VisualTestContext, point, px, size,
};
use riso_ui::{Tab, TabBar, v_flex};

// `TAB-<id>` comes from the tab element. Twenty `tNN` tabs are far wider than
// a 640px bar: with `track_scroll` they sit in a row that scrolls along the
// handle, as GPUI's `overflow_x_scroll` does; without it the bar lays them
// out as before, cut off at the end.
struct Bar {
    handle: Option<ScrollHandle>,
}

impl gpui::Render for Bar {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let bar = TabBar::new("bar").tabs(
            (1..=20)
                .map(|index| Tab::new(format!("t{index:02}"), format!("t{index:02}")))
                .collect(),
        );
        v_flex()
            .size_full()
            .items_start()
            .child(match &self.handle {
                Some(handle) => bar.track_scroll(handle),
                None => bar,
            })
    }
}

fn draw_a_frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn open(cx: &mut TestAppContext, bar: Bar) -> VisualTestContext {
    cx.update(riso_ui::init);
    let window = cx.open_window(size(px(640.), px(480.)), |_, _| bar);
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    draw_a_frame(&mut cx);
    cx
}

#[gpui::test]
fn a_bar_with_track_scroll_and_more_tabs_than_fit_has_a_scrollable_row(cx: &mut TestAppContext) {
    let handle = ScrollHandle::new();
    let mut cx = open(
        cx,
        Bar {
            handle: Some(handle.clone()),
        },
    );
    assert_eq!(
        handle.offset().x,
        px(0.),
        "the row starts unscrolled: {:?}",
        handle.offset()
    );
    assert!(
        handle.max_offset().x > px(0.),
        "more tabs than fit gives the row something to scroll: {:?}",
        handle.max_offset()
    );
    let last = cx.debug_bounds("TAB-t20").expect("the last tab draws");
    assert!(
        last.origin.x > px(640.),
        "tabs that do not fit run past the bar's end: {last:?}"
    );
}

#[gpui::test]
fn a_horizontal_scroll_moves_the_row(cx: &mut TestAppContext) {
    let handle = ScrollHandle::new();
    let mut cx = open(
        cx,
        Bar {
            handle: Some(handle.clone()),
        },
    );
    let over = cx.debug_bounds("TAB-t01").expect("a tab draws").center();
    cx.simulate_event(ScrollEvent {
        position: over,
        delta: ScrollDelta::Pixels(point(px(-60.), px(0.))),
        ..Default::default()
    });
    draw_a_frame(&mut cx);
    assert_eq!(
        handle.offset().x,
        px(-60.),
        "a horizontal gesture scrolls the row: {:?}",
        handle.offset()
    );
    let first = cx.debug_bounds("TAB-t01").expect("the first tab draws");
    assert!(
        first.origin.x < px(0.),
        "the tabs moved with the row: {first:?}"
    );
}

#[gpui::test]
fn a_vertical_wheel_over_the_bar_moves_the_row_sideways(cx: &mut TestAppContext) {
    let handle = ScrollHandle::new();
    let mut cx = open(
        cx,
        Bar {
            handle: Some(handle.clone()),
        },
    );
    let over = cx.debug_bounds("TAB-t01").expect("a tab draws").center();
    cx.simulate_event(ScrollEvent {
        position: over,
        delta: ScrollDelta::Pixels(point(px(0.), px(-60.))),
        ..Default::default()
    });
    draw_a_frame(&mut cx);
    assert_eq!(
        handle.offset().x,
        px(-60.),
        "a vertical wheel over the bar scrolls the row sideways: {:?}",
        handle.offset()
    );
    assert_eq!(
        handle.offset().y,
        px(0.),
        "nothing scrolls vertically: {:?}",
        handle.offset()
    );
}

#[gpui::test]
fn a_bar_without_track_scroll_lays_its_tabs_out_as_today(cx: &mut TestAppContext) {
    let mut cx = open(cx, Bar { handle: None });
    let first = cx.debug_bounds("TAB-t01").expect("the first tab draws");
    let last = cx.debug_bounds("TAB-t20").expect("the last tab draws");
    assert_eq!(
        first.origin.x,
        px(0.),
        "the first tab sits at the bar's start: {first:?}"
    );
    assert!(
        last.origin.x > px(640.),
        "tabs that do not fit are cut off at the bar's end: {last:?}"
    );
}
