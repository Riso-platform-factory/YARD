#![cfg(feature = "dev")]

use gpui::{
    InteractiveElement, ParentElement, Styled, TestAppContext, VisualTestContext, div, px, size,
};
use riso_ui::dev::{List, ListHeader, ListItem, ListItemSpacing, ListSeparator};
use riso_ui::{Label, v_flex};
use std::{cell::Cell, rc::Rc};
use theme::ActiveTheme;

// A window of a known size: 400px wide, 600px tall.
fn open_test_window(
    view: impl gpui::Render + 'static,
    cx: &mut TestAppContext,
) -> VisualTestContext {
    let window = cx.open_window(size(px(400.), px(600.)), |_, _| view);
    VisualTestContext::from_window(window.into(), cx)
}

// Draws a frame so `debug_bounds` sees the wrapped elements.
fn draw_a_frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

// Draws a frame so `debug_bounds` sees the wrapped element, then clicks its center.
fn click_center(cx: &mut VisualTestContext, selector: &'static str) {
    draw_a_frame(cx);
    let bounds = cx.debug_bounds(selector).expect(selector);
    cx.simulate_click(bounds.center(), gpui::Modifiers::default());
}

// The painted quads that occupy exactly `bounds`. Quad bounds are in scaled
// pixels, so each edge is converted back to pixels with the window's scale
// factor before it is compared.
fn quads_on(
    quads: &[gpui::Quad],
    bounds: gpui::Bounds<gpui::Pixels>,
    scale_factor: f32,
) -> Vec<gpui::Quad> {
    quads
        .iter()
        .filter(|quad| {
            let in_pixels = quad.bounds.map(|edge| px(edge.as_f32() / scale_factor));
            (f32::from(in_pixels.origin.x) - f32::from(bounds.origin.x)).abs() <= 0.5
                && (f32::from(in_pixels.origin.y) - f32::from(bounds.origin.y)).abs() <= 0.5
                && (f32::from(in_pixels.size.width) - f32::from(bounds.size.width)).abs() <= 0.5
                && (f32::from(in_pixels.size.height) - f32::from(bounds.size.height)).abs() <= 0.5
        })
        .copied()
        .collect()
}

struct Ordered;

impl gpui::Render for Ordered {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex().size_full().child(
            div().debug_selector(|| "LIST".into()).child(
                List::new()
                    .header(
                        ListHeader::new("Section")
                            .start_slot(div().debug_selector(|| "HEADER_MARK".into()).size_4()),
                    )
                    .child(
                        div()
                            .debug_selector(|| "ITEM_0".into())
                            .child(ListItem::new("first").child(Label::new("First"))),
                    )
                    .child(
                        div()
                            .debug_selector(|| "ITEM_1".into())
                            .child(ListItem::new("second").child(Label::new("Second"))),
                    )
                    .child(
                        div()
                            .debug_selector(|| "ITEM_2".into())
                            .child(ListItem::new("third").child(Label::new("Third"))),
                    ),
            ),
        )
    }
}

#[gpui::test]
fn the_header_is_above_the_first_item_the_items_are_in_order_and_each_row_is_as_wide_as_the_list(
    cx: &mut TestAppContext,
) {
    cx.update(riso_ui::init);
    let mut cx = open_test_window(Ordered, cx);
    draw_a_frame(&mut cx);
    let list = cx.debug_bounds("LIST").expect("the list draws");
    let header = cx
        .debug_bounds("HEADER_MARK")
        .expect("a marker in the header's start slot draws");
    let first = cx.debug_bounds("ITEM_0").expect("the first item draws");
    let second = cx.debug_bounds("ITEM_1").expect("the second item draws");
    let third = cx.debug_bounds("ITEM_2").expect("the third item draws");
    assert!(
        header.top() >= list.top() && header.bottom() <= first.top(),
        "the header draws inside the list, above the first item: {header:?} vs {first:?}"
    );
    assert!(
        second.top() >= first.bottom() && third.top() >= second.bottom(),
        "the items draw in order: {first:?} {second:?} {third:?}"
    );
    for (selector, item) in [("ITEM_0", first), ("ITEM_1", second), ("ITEM_2", third)] {
        assert_eq!(
            item.size.width, list.size.width,
            "{selector} is as wide as the list"
        );
    }
}

struct Spaced;

impl gpui::Render for Spaced {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex().size_full().child(
            List::new()
                .child(
                    div().debug_selector(|| "EXTRA_DENSE".into()).child(
                        ListItem::new("extra-dense")
                            .spacing(ListItemSpacing::ExtraDense)
                            .child(Label::new("Row")),
                    ),
                )
                .child(
                    div()
                        .debug_selector(|| "DENSE".into())
                        .child(ListItem::new("dense").child(Label::new("Row"))),
                )
                .child(
                    div().debug_selector(|| "SPARSE".into()).child(
                        ListItem::new("sparse")
                            .spacing(ListItemSpacing::Sparse)
                            .child(Label::new("Row")),
                    ),
                ),
        )
    }
}

#[gpui::test]
fn the_three_spacings_draw_extra_dense_shorter_than_dense_shorter_than_sparse(
    cx: &mut TestAppContext,
) {
    cx.update(riso_ui::init);
    let mut cx = open_test_window(Spaced, cx);
    draw_a_frame(&mut cx);
    let extra_dense = cx
        .debug_bounds("EXTRA_DENSE")
        .expect("the extra dense item draws");
    let dense = cx.debug_bounds("DENSE").expect("the dense item draws");
    let sparse = cx.debug_bounds("SPARSE").expect("the sparse item draws");
    assert!(
        extra_dense.size.height < dense.size.height,
        "extra dense is shorter than dense: {extra_dense:?} vs {dense:?}"
    );
    assert!(
        dense.size.height < sparse.size.height,
        "dense is shorter than sparse: {dense:?} vs {sparse:?}"
    );
    // Zed's heights: extra dense pads -1px above and below, sparse pads
    // 0.25rem (4px at the default 16px rem) above and below, dense pads nothing.
    let dense_over_extra = dense.size.height - extra_dense.size.height;
    assert!(
        (f32::from(dense_over_extra) - 2.).abs() <= 0.5,
        "extra dense is 2px shorter than dense: {dense_over_extra:?}"
    );
    let sparse_over_dense = sparse.size.height - dense.size.height;
    assert!(
        (f32::from(sparse_over_dense) - 8.).abs() <= 0.5,
        "sparse is 8px taller than dense: {sparse_over_dense:?}"
    );
}

struct Empty;

impl gpui::Render for Empty {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex()
            .size_full()
            .child(
                div()
                    .debug_selector(|| "EMPTY_DEFAULT".into())
                    .child(List::new()),
            )
            .child(
                div()
                    .debug_selector(|| "EMPTY_CUSTOM".into())
                    .child(List::new().empty_message("No themes installed")),
            )
    }
}

#[gpui::test]
fn an_empty_list_draws_its_empty_message_with_the_default_and_a_given_one(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let mut cx = open_test_window(Empty, cx);
    draw_a_frame(&mut cx);
    for (selector, name) in [
        ("EMPTY_DEFAULT", "the default `No items`"),
        ("EMPTY_CUSTOM", "a given message"),
    ] {
        let list = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{name} draws"));
        assert!(
            list.size.height > px(8.),
            "{name} draws a line of text under the list's 4px vertical padding: {list:?}"
        );
    }
}

struct Clicks {
    clicks: Rc<Cell<usize>>,
}

impl gpui::Render for Clicks {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        let enabled = self.clicks.clone();
        let disabled = self.clicks.clone();
        v_flex().size_full().child(
            List::new()
                .child(
                    div().debug_selector(|| "ENABLED_ITEM".into()).child(
                        ListItem::new("enabled-item")
                            .child(Label::new("Choose"))
                            .on_click(move |_, _| enabled.set(enabled.get() + 1)),
                    ),
                )
                .child(
                    div().debug_selector(|| "DISABLED_ITEM".into()).child(
                        ListItem::new("disabled-item")
                            .disabled(true)
                            .child(Label::new("Nope"))
                            .on_click(move |_, _| disabled.set(disabled.get() + 1)),
                    ),
                ),
        )
    }
}

#[gpui::test]
fn a_click_on_an_enabled_item_runs_its_handler_once_per_click_and_a_disabled_items_never_runs(
    cx: &mut TestAppContext,
) {
    cx.update(riso_ui::init);
    let clicks = Rc::new(Cell::new(0));
    let counted = clicks.clone();
    let mut cx = open_test_window(Clicks { clicks: counted }, cx);
    click_center(&mut cx, "ENABLED_ITEM");
    assert_eq!(clicks.get(), 1);
    click_center(&mut cx, "ENABLED_ITEM");
    assert_eq!(clicks.get(), 2, "each click runs the handler once");
    click_center(&mut cx, "DISABLED_ITEM");
    click_center(&mut cx, "DISABLED_ITEM");
    assert_eq!(clicks.get(), 2, "a disabled item's handler never runs");
}

struct Slotted;

impl gpui::Render for Slotted {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex().size_full().child(
            List::new().child(
                div().debug_selector(|| "SLOTTED".into()).child(
                    ListItem::new("slotted")
                        .start_slot(div().debug_selector(|| "SLOT_START".into()).size_4())
                        .child(
                            div()
                                .debug_selector(|| "SLOT_TEXT".into())
                                .child(Label::new("Body")),
                        )
                        .end_slot(div().debug_selector(|| "SLOT_END".into()).size_4()),
                ),
            ),
        )
    }
}

#[gpui::test]
fn the_start_slot_is_left_of_the_text_and_the_end_slot_is_at_the_rows_right_edge(
    cx: &mut TestAppContext,
) {
    cx.update(riso_ui::init);
    let mut cx = open_test_window(Slotted, cx);
    draw_a_frame(&mut cx);
    let row = cx.debug_bounds("SLOTTED").expect("the row draws");
    let start = cx.debug_bounds("SLOT_START").expect("the start slot draws");
    let text = cx.debug_bounds("SLOT_TEXT").expect("the text draws");
    let end = cx.debug_bounds("SLOT_END").expect("the end slot draws");
    assert!(
        start.left() >= row.left() && start.right() <= text.left(),
        "the start slot draws inside the row, left of the text: {start:?} vs {text:?}"
    );
    assert!(
        end.left() >= text.right(),
        "the end slot draws right of the text: {end:?} vs {text:?}"
    );
    // Zed pads the row's content horizontally by DynamicSpacing::Base06, which
    // is 6px at the default UI density.
    let gap_to_right_edge = row.right() - end.right();
    assert!(
        (f32::from(gap_to_right_edge) - 6.).abs() <= 1.,
        "the end slot sits at the row's right edge minus Zed's 6px padding: {end:?} vs {row:?}"
    );
}

struct Chosen;

impl gpui::Render for Chosen {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex().size_full().child(
            List::new()
                .child(
                    div()
                        .debug_selector(|| "PLAIN".into())
                        .child(ListItem::new("plain").child(Label::new("Plain"))),
                )
                .child(
                    div().debug_selector(|| "CHOSEN".into()).child(
                        ListItem::new("chosen")
                            .toggle_state(true)
                            .child(Label::new("Chosen")),
                    ),
                ),
        )
    }
}

#[gpui::test]
fn a_selected_item_and_an_unselected_one_draw_different_backgrounds(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let mut cx = open_test_window(Chosen, cx);
    draw_a_frame(&mut cx);
    let plain = cx.debug_bounds("PLAIN").expect("the unselected item draws");
    let chosen = cx.debug_bounds("CHOSEN").expect("the selected item draws");
    let (quads, scale_factor) =
        cx.update(|window, _| (window.painted_quads(), window.scale_factor()));
    let selected_color = cx.update(|_, cx| cx.theme().colors().ghost_element_selected);
    let plain_colors: Vec<gpui::Hsla> = quads_on(&quads, plain, scale_factor)
        .iter()
        .filter_map(|quad| quad.background.as_solid())
        .collect();
    let chosen_colors: Vec<gpui::Hsla> = quads_on(&quads, chosen, scale_factor)
        .iter()
        .filter_map(|quad| quad.background.as_solid())
        .collect();
    assert!(
        chosen_colors.contains(&selected_color),
        "the selected item paints Zed's selected background over its row: {chosen_colors:?}"
    );
    assert!(
        !plain_colors.contains(&selected_color),
        "the unselected item does not: {plain_colors:?}"
    );
}

struct Nested;

impl gpui::Render for Nested {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex().size_full().child(
            List::new()
                .child(
                    ListItem::new("root").child(
                        div()
                            .debug_selector(|| "LEVEL_0_TEXT".into())
                            .child(Label::new("Row")),
                    ),
                )
                .child(
                    ListItem::new("nested").indent_level(2).child(
                        div()
                            .debug_selector(|| "LEVEL_2_TEXT".into())
                            .child(Label::new("Row")),
                    ),
                ),
        )
    }
}

#[gpui::test]
fn indent_level_2_moves_the_items_content_right_by_two_of_zeds_steps(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let mut cx = open_test_window(Nested, cx);
    draw_a_frame(&mut cx);
    let level_0 = cx
        .debug_bounds("LEVEL_0_TEXT")
        .expect("the level 0 item's text draws");
    let level_2 = cx
        .debug_bounds("LEVEL_2_TEXT")
        .expect("the level 2 item's text draws");
    // Zed's default indent step is 12px; two levels move the content 24px right.
    let shift = level_2.left() - level_0.left();
    assert!(
        (f32::from(shift) - 24.).abs() <= 0.5,
        "indent_level(2) moves the content right by two 12px steps: {level_0:?} vs {level_2:?}"
    );
}

struct Separated;

impl gpui::Render for Separated {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex().size_full().child(
            div().debug_selector(|| "SEPARATED_LIST".into()).child(
                List::new()
                    .child(
                        div()
                            .debug_selector(|| "ABOVE".into())
                            .child(ListItem::new("above").child(Label::new("Above"))),
                    )
                    .child(ListSeparator)
                    .child(
                        div()
                            .debug_selector(|| "BELOW".into())
                            .child(ListItem::new("below").child(Label::new("Below"))),
                    ),
            ),
        )
    }
}

#[gpui::test]
fn a_separator_draws_zeds_thin_line_between_two_items(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let mut cx = open_test_window(Separated, cx);
    draw_a_frame(&mut cx);
    let list = cx.debug_bounds("SEPARATED_LIST").expect("the list draws");
    let above = cx.debug_bounds("ABOVE").expect("the upper item draws");
    let below = cx.debug_bounds("BELOW").expect("the lower item draws");
    let (quads, scale_factor) =
        cx.update(|window, _| (window.painted_quads(), window.scale_factor()));
    let border_variant = cx.update(|_, cx| cx.theme().colors().border_variant);
    let line = quads.iter().copied().find(|quad| {
        let bounds = quad.bounds.map(|edge| px(edge.as_f32() / scale_factor));
        quad.background.as_solid() == Some(border_variant)
            && (f32::from(bounds.size.height) - 1.).abs() <= 0.5
            && (f32::from(bounds.size.width) - f32::from(list.size.width)).abs() <= 0.5
            && bounds.origin.y >= above.bottom()
            && bounds.origin.y + bounds.size.height <= below.top()
    });
    assert!(
        line.is_some(),
        "a 1px line in the border_variant color spans the row between the items: {quads:?}"
    );
}
