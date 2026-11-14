use gpui::{
    InteractiveElement, IntoElement, Modifiers, MouseButton, ParentElement, ScrollDelta,
    ScrollEvent, ScrollHandle, ScrollStrategy, Styled, TestAppContext, UniformListScrollHandle,
    VisualTestContext, div, point, px, size,
};
use riso_ui::{ScrollArea, ScrollList, h_flex, v_flex};
use std::{cell::RefCell, ops::Range, rc::Rc};

// A window of a known size: 400px wide, 600px tall.
fn open_test_window(
    view: impl gpui::Render + 'static,
    cx: &mut TestAppContext,
) -> VisualTestContext {
    let window = cx.open_window(size(px(400.), px(600.)), |_, _| view);
    VisualTestContext::from_window(window.into(), cx)
}

// Draws a frame so `debug_bounds` and the painted quads see the elements.
fn draw_a_frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

// The painted quads' bounds in pixels: quad bounds are in scaled pixels, so
// each edge is converted back with the window's scale factor.
fn painted_bounds(cx: &mut VisualTestContext) -> Vec<gpui::Bounds<gpui::Pixels>> {
    let (quads, scale_factor) =
        cx.update(|window, _| (window.painted_quads(), window.scale_factor()));
    quads
        .iter()
        .map(|quad| quad.bounds.map(|edge| px(edge.as_f32() / scale_factor)))
        .collect()
}

// The narrow quads inside `area`'s right edge strip: Zed's regular scrollbar
// is a 6px thumb inside a 14px track, and it is the only narrow quad there.
fn thumbs_at_right_edge(
    area: gpui::Bounds<gpui::Pixels>,
    painted: &[gpui::Bounds<gpui::Pixels>],
) -> Vec<gpui::Bounds<gpui::Pixels>> {
    painted
        .iter()
        .copied()
        .filter(|bounds| {
            bounds.size.width <= px(14.)
                && bounds.size.height >= px(10.)
                && bounds.origin.x >= area.right() - px(15.)
                && bounds.origin.x + bounds.size.width <= area.right() + px(0.5)
                && bounds.origin.y >= area.top() - px(0.5)
                && bounds.origin.y + bounds.size.height <= area.bottom() + px(0.5)
        })
        .collect()
}

struct Area {
    handle: ScrollHandle,
}

impl gpui::Render for Area {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex().size_full().child(
            div().debug_selector(|| "AREA".into()).child(
                ScrollArea::new("area", &self.handle).h(px(100.)).child(
                    v_flex()
                        .child(div().debug_selector(|| "TOP_MARK".into()).h(px(10.)))
                        .child(div().h(px(480.)))
                        .child(div().debug_selector(|| "BOTTOM_MARK".into()).h(px(10.))),
                ),
            ),
        )
    }
}

#[gpui::test]
fn the_wheel_moves_an_areas_content_and_the_handle_follows_until_the_content_stops_at_its_ends(
    cx: &mut TestAppContext,
) {
    cx.update(riso_ui::init);
    let handle = ScrollHandle::new();
    let mut cx = open_test_window(
        Area {
            handle: handle.clone(),
        },
        cx,
    );
    draw_a_frame(&mut cx);
    let area = cx.debug_bounds("AREA").expect("the area draws");
    let top = cx
        .debug_bounds("TOP_MARK")
        .expect("the content's top draws");
    assert_eq!(handle.offset().y, px(0.), "the area starts unscrolled");
    assert!(
        (f32::from(top.origin.y) - f32::from(area.origin.y)).abs() <= 0.5,
        "the content's top sits at the area's top: {top:?} vs {area:?}"
    );

    // One wheel step moves the content up by the delta; the handle follows.
    cx.simulate_event(ScrollEvent {
        position: area.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-60.))),
        ..Default::default()
    });
    draw_a_frame(&mut cx);
    assert_eq!(
        handle.offset().y,
        px(-60.),
        "the handle's offset follows the wheel"
    );
    let top = cx
        .debug_bounds("TOP_MARK")
        .expect("the content's top draws");
    assert!(
        (f32::from(top.origin.y) - (f32::from(area.origin.y) - 60.)).abs() <= 0.5,
        "the content moved up by the wheel delta: {top:?} vs {area:?}"
    );

    // 500px of content in a 100px area leaves 400px to scroll; at the end the
    // content's bottom sits at the area's bottom and scrolling goes no further.
    cx.simulate_event(ScrollEvent {
        position: area.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-10_000.))),
        ..Default::default()
    });
    draw_a_frame(&mut cx);
    assert_eq!(
        handle.offset().y,
        px(-400.),
        "the offset clamps at the content's end"
    );
    let bottom = cx
        .debug_bounds("BOTTOM_MARK")
        .expect("the content's bottom draws");
    assert!(
        (f32::from(bottom.bottom()) - f32::from(area.bottom())).abs() <= 0.5,
        "the content's bottom sits at the area's bottom: {bottom:?} vs {area:?}"
    );
    cx.simulate_event(ScrollEvent {
        position: area.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-10_000.))),
        ..Default::default()
    });
    draw_a_frame(&mut cx);
    assert_eq!(
        handle.offset().y,
        px(-400.),
        "scrolling further does not pass the content's end"
    );

    // Back at the start the content stops at the top again.
    cx.simulate_event(ScrollEvent {
        position: area.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(10_000.))),
        ..Default::default()
    });
    draw_a_frame(&mut cx);
    assert_eq!(
        handle.offset().y,
        px(0.),
        "scrolling back up returns the offset to zero"
    );
    let top = cx
        .debug_bounds("TOP_MARK")
        .expect("the content's top draws");
    assert!(
        (f32::from(top.origin.y) - f32::from(area.origin.y)).abs() <= 0.5,
        "the content's top is back at the area's top: {top:?} vs {area:?}"
    );
    cx.simulate_event(ScrollEvent {
        position: area.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(10_000.))),
        ..Default::default()
    });
    draw_a_frame(&mut cx);
    assert_eq!(
        handle.offset().y,
        px(0.),
        "scrolling up further does not pass the content's start"
    );
}

struct TwoAreas {
    fitting: ScrollHandle,
    overflowing: ScrollHandle,
}

impl gpui::Render for TwoAreas {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex().size_full().child(
            h_flex()
                .child(
                    div().debug_selector(|| "FITS".into()).child(
                        ScrollArea::new("fits", &self.fitting)
                            .w(px(150.))
                            .h(px(100.))
                            .child(div().w_full().h(px(50.)).bg(gpui::blue())),
                    ),
                )
                .child(
                    div().debug_selector(|| "OVERFLOWS".into()).child(
                        ScrollArea::new("overflows", &self.overflowing)
                            .w(px(150.))
                            .h(px(100.))
                            .child(div().w_full().h(px(500.)).bg(gpui::green())),
                    ),
                ),
        )
    }
}

#[gpui::test]
fn an_area_whose_content_fits_paints_no_scrollbar_thumb(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let mut cx = open_test_window(
        TwoAreas {
            fitting: ScrollHandle::new(),
            overflowing: ScrollHandle::new(),
        },
        cx,
    );
    // The thumb fades in and out with the system's scrollbar setting; reduced
    // motion settles that in the same frame, so a shown thumb is in the quads.
    cx.update(|_, cx| cx.set_reduce_motion(true));
    draw_a_frame(&mut cx);
    let fitting = cx.debug_bounds("FITS").expect("the fitting area draws");
    let overflowing = cx
        .debug_bounds("OVERFLOWS")
        .expect("the overflowing area draws");
    let painted = painted_bounds(&mut cx);
    let fitting_thumbs = thumbs_at_right_edge(fitting, &painted);
    let overflowing_thumbs = thumbs_at_right_edge(overflowing, &painted);
    assert!(
        !overflowing_thumbs.is_empty(),
        "the overflowing area paints a thumb at its right edge: {overflowing_thumbs:?}"
    );
    assert!(
        fitting_thumbs.is_empty(),
        "the area whose content fits paints no scrollbar thumb: {fitting_thumbs:?}"
    );
}

struct LongList {
    handle: UniformListScrollHandle,
    ranges: Rc<RefCell<Vec<Range<usize>>>>,
}

impl gpui::Render for LongList {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        let ranges = self.ranges.clone();
        v_flex().size_full().child(
            div().debug_selector(|| "LIST".into()).w(px(300.)).child(
                ScrollList::new(
                    "rows",
                    1000,
                    &self.handle,
                    move |range: Range<usize>, _: &mut gpui::Window, _: &mut gpui::App| {
                        ranges.borrow_mut().push(range.clone());
                        range
                            .map(|ix| {
                                div()
                                    .h(px(20.))
                                    .child(format!("Row {ix}"))
                                    .into_any_element()
                            })
                            .collect()
                    },
                )
                .h(px(200.))
                .w(px(300.)),
            ),
        )
    }
}

#[gpui::test]
fn a_list_of_a_thousand_rows_asks_only_for_the_rows_in_view_and_the_range_reaches_the_last_row_at_the_end(
    cx: &mut TestAppContext,
) {
    cx.update(riso_ui::init);
    let ranges = Rc::new(RefCell::new(Vec::new()));
    let mut cx = open_test_window(
        LongList {
            handle: UniformListScrollHandle::new(),
            ranges: ranges.clone(),
        },
        cx,
    );
    draw_a_frame(&mut cx);
    {
        let ranges = ranges.borrow();
        let visible = ranges.last().expect("a drawn frame asks for rows");
        assert_eq!(
            *visible,
            0..10,
            "ten 20px rows fill the 200px list: {ranges:?}"
        );
        assert!(
            ranges.iter().all(|range| range.len() <= 11),
            "every asked range is at most the ten visible plus GPUI's overdraw: {ranges:?}"
        );
    }

    let list = cx.debug_bounds("LIST").expect("the list draws");
    cx.simulate_event(ScrollEvent {
        position: list.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-25_000.))),
        ..Default::default()
    });
    draw_a_frame(&mut cx);
    let ranges = ranges.borrow();
    let visible = ranges.last().expect("the scrolled frame asks for rows");
    assert_eq!(
        visible.end, 1000,
        "scrolled to the end, the asked range reaches the last row: {visible:?}"
    );
    assert!(
        ranges.iter().all(|range| range.len() <= 11),
        "scrolled or not, every asked range stays small: {ranges:?}"
    );
}

#[gpui::test]
fn scroll_to_item_brings_the_row_into_view_at_the_next_frame(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let ranges = Rc::new(RefCell::new(Vec::new()));
    let handle = UniformListScrollHandle::new();
    let mut cx = open_test_window(
        LongList {
            handle: handle.clone(),
            ranges: ranges.clone(),
        },
        cx,
    );
    draw_a_frame(&mut cx);
    handle.scroll_to_item(500, ScrollStrategy::Top);
    draw_a_frame(&mut cx);
    let visible = ranges
        .borrow()
        .last()
        .cloned()
        .expect("the next frame asks for rows");
    assert!(
        visible.start <= 500 && visible.end > 500,
        "row 500 is in view at the next frame: {visible:?}"
    );
}

#[gpui::test]
fn dragging_the_lists_thumb_moves_the_visible_range(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let ranges = Rc::new(RefCell::new(Vec::new()));
    let mut cx = open_test_window(
        LongList {
            handle: UniformListScrollHandle::new(),
            ranges: ranges.clone(),
        },
        cx,
    );
    // As above: reduced motion settles the thumb's fade in the same frame.
    cx.update(|_, cx| cx.set_reduce_motion(true));
    draw_a_frame(&mut cx);
    let list = cx.debug_bounds("LIST").expect("the list draws");
    let painted = painted_bounds(&mut cx);
    let thumbs = thumbs_at_right_edge(list, &painted);
    let thumb = thumbs
        .first()
        .unwrap_or_else(|| panic!("the list's thumb paints at its right edge: {painted:?}"));
    let grab = thumb.center();

    cx.simulate_mouse_down(grab, MouseButton::Left, Modifiers::default());
    let release = grab + point(px(0.), px(80.));
    cx.simulate_mouse_move(release, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(release, MouseButton::Left, Modifiers::default());
    draw_a_frame(&mut cx);

    let visible = ranges
        .borrow()
        .last()
        .cloned()
        .expect("the frame after the drag asks for rows");
    assert!(
        visible.start > 0,
        "dragging the thumb down moved the visible range: {visible:?}"
    );
}
