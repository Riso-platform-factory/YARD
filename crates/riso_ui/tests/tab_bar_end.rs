use gpui::{
    InteractiveElement, IntoElement, ParentElement, ScrollDelta, ScrollEvent, ScrollHandle, Styled,
    TestAppContext, VisualTestContext, point, px, size,
};
use riso_ui::{Tab, TabBar, v_flex};

// `TAB-<id>` comes from the tab element. The end child the tests pass is a
// 30px box marked `END`, so `debug_bounds` finds it. Three `tNN` tabs fit in
// a 640px bar; twenty do not.
struct Bar {
    handle: ScrollHandle,
    tabs: usize,
    end: bool,
}

impl gpui::Render for Bar {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let bar = TabBar::new("bar")
            .tabs(
                (1..=self.tabs)
                    .map(|index| Tab::new(format!("t{index:02}"), format!("t{index:02}")))
                    .collect(),
            )
            .track_scroll(&self.handle);
        v_flex().size_full().items_start().child(if self.end {
            bar.end_child(
                gpui::div()
                    .debug_selector(|| "END".to_string())
                    .w(px(30.))
                    .h_full()
                    .child("+"),
            )
        } else {
            bar
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

// An `end_child` is drawn after the tab row, at the bar's end, in view.
#[gpui::test]
fn an_end_child_draws_at_the_bars_end_after_the_tabs(cx: &mut TestAppContext) {
    let mut cx = open(
        cx,
        Bar {
            handle: ScrollHandle::new(),
            tabs: 3,
            end: true,
        },
    );
    let first = cx.debug_bounds("TAB-t01").expect("the first tab draws");
    let last = cx.debug_bounds("TAB-t03").expect("the last tab draws");
    let end = cx.debug_bounds("END").expect("the end child draws");
    assert_eq!(
        first.origin.x,
        px(0.),
        "the tabs start at the bar's start: {first:?}"
    );
    assert!(
        end.origin.x > last.right(),
        "the end child is after the tabs: {end:?} vs {last:?}"
    );
    assert!(
        end.right() > px(600.) && end.right() <= px(640.),
        "the end child is at the bar's end, in view: {end:?}"
    );
}

// The end child is outside the scrolling row: a scroll moves the tabs and
// leaves it whole, in place.
#[gpui::test]
fn the_end_child_stays_in_view_while_the_row_scrolls(cx: &mut TestAppContext) {
    let handle = ScrollHandle::new();
    let mut cx = open(
        cx,
        Bar {
            handle: handle.clone(),
            tabs: 20,
            end: true,
        },
    );
    let end = cx.debug_bounds("END").expect("the end child draws");
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
    let after = cx.debug_bounds("END").expect("the end child still draws");
    assert_eq!(
        after, end,
        "the end child stayed in place, outside the scrolling row: {after:?} vs {end:?}"
    );
    assert!(
        after.right() <= px(640.),
        "and still in view at the bar's end: {after:?}"
    );
}

#[gpui::test]
fn a_bar_without_an_end_child_lays_out_as_today(cx: &mut TestAppContext) {
    let mut cx = open(
        cx,
        Bar {
            handle: ScrollHandle::new(),
            tabs: 20,
            end: false,
        },
    );
    assert!(
        cx.debug_bounds("END").is_none(),
        "a bar without an end child draws none"
    );
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
