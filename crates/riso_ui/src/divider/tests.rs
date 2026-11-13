use super::*;
use crate::{h_flex, v_flex};
use gpui::{InteractiveElement, IntoElement, ParentElement, Styled, div, px};
use theme::ActiveTheme;

const COLORS: [DividerColor; 3] = [
    DividerColor::Border,
    DividerColor::BorderFaded,
    DividerColor::BorderVariant,
];

fn bounds(x: f32, y: f32, width: f32, height: f32) -> gpui::Bounds<gpui::Pixels> {
    gpui::Bounds {
        origin: gpui::point(px(x), px(y)),
        size: gpui::size(px(width), px(height)),
    }
}

#[test]
fn the_default_color_is_border_variant() {
    assert_eq!(DividerColor::default(), DividerColor::BorderVariant);
}

// A DividerColor's color, mapped as the contract says: the theme's border
// color, the same at 60%, and its variant border color.
fn themed(cx: &gpui::App, color: DividerColor) -> gpui::Hsla {
    let colors = cx.theme().colors();
    match color {
        DividerColor::Border => colors.border,
        DividerColor::BorderFaded => colors.border.opacity(0.6),
        DividerColor::BorderVariant => colors.border_variant,
    }
}

#[gpui::test]
fn every_divider_color_is_the_themes_under_one_dark(cx: &mut gpui::TestAppContext) {
    cx.update(crate::init);
    cx.read(|cx| {
        assert_eq!(crate::active_theme(cx), "One Dark");
        for color in COLORS {
            assert_eq!(
                color.hsla(cx),
                themed(cx, color),
                "{color:?} under One Dark"
            );
        }
    });
}

// A solid divider's line is the one quad its draw leaves in the scene.
fn painted_line(window: &gpui::Window) -> gpui::Bounds<gpui::ScaledPixels> {
    let quads = window.painted_quads();
    assert_eq!(quads.len(), 1, "a solid divider paints one quad: {quads:?}");
    quads[0].bounds
}

struct Column {
    inset: bool,
}

impl gpui::Render for Column {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        v_flex()
            .w(px(200.))
            .child(
                div()
                    .debug_selector(|| "DIVIDER".into())
                    .child(match self.inset {
                        true => Divider::horizontal().inset(),
                        false => Divider::horizontal(),
                    }),
            )
    }
}

#[gpui::test]
fn a_horizontal_divider_in_a_column_fills_its_width_and_is_one_pixel_high(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::init);
    let (_view, cx) = cx.add_window_view(|_, _| Column { inset: false });
    cx.update(|window, cx| {
        window.refresh();
        let _ = window.draw(cx);
        let line = painted_line(window);
        assert_eq!(line, bounds(0., 0., 200., 1.).scale(window.scale_factor()));
    });
    assert_eq!(cx.debug_bounds("DIVIDER"), Some(bounds(0., 0., 200., 1.)));
}

#[gpui::test]
fn an_inset_horizontal_divider_is_narrower_than_its_column(cx: &mut gpui::TestAppContext) {
    cx.update(crate::init);
    let (_view, cx) = cx.add_window_view(|_, _| Column { inset: true });
    cx.update(|window, cx| {
        window.refresh();
        let _ = window.draw(cx);
        let line = painted_line(window);
        assert_eq!(
            line.size.height,
            px(1.).scale(window.scale_factor()),
            "an inset divider is still one pixel high"
        );
        let right = line.origin.x + line.size.width;
        assert!(
            line.origin.x > gpui::ScaledPixels(0.) && right < px(200.).scale(window.scale_factor()),
            "a margin at both ends: {line:?} in a column 200px wide"
        );
    });
}

struct Row;

impl gpui::Render for Row {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        h_flex().h(px(80.)).child(
            div()
                .h_full()
                .debug_selector(|| "DIVIDER".into())
                .child(Divider::vertical()),
        )
    }
}

#[gpui::test]
fn a_vertical_divider_in_a_row_fills_its_height_and_is_one_pixel_wide(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::init);
    let (_view, cx) = cx.add_window_view(|_, _| Row);
    cx.update(|window, cx| {
        window.refresh();
        let _ = window.draw(cx);
        let line = painted_line(window);
        assert_eq!(line, bounds(0., 0., 1., 80.).scale(window.scale_factor()));
    });
    assert_eq!(cx.debug_bounds("DIVIDER"), Some(bounds(0., 0., 1., 80.)));
}

struct Dashed;

impl gpui::Render for Dashed {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        v_flex()
            .w(px(200.))
            .child(
                div()
                    .debug_selector(|| "DASHED_HORIZONTAL".into())
                    .child(Divider::horizontal_dashed()),
            )
            .child(
                h_flex().h(px(80.)).child(
                    div()
                        .h_full()
                        .debug_selector(|| "DASHED_VERTICAL".into())
                        .child(Divider::vertical_dashed()),
                ),
            )
    }
}

#[gpui::test]
fn a_dashed_divider_of_each_direction_draws_in_its_stack(cx: &mut gpui::TestAppContext) {
    cx.update(crate::init);
    let (_view, cx) = cx.add_window_view(|_, _| Dashed);
    cx.update(|window, cx| {
        window.refresh();
        let _ = window.draw(cx);
    });
    assert_eq!(
        cx.debug_bounds("DASHED_HORIZONTAL"),
        Some(bounds(0., 0., 200., 1.))
    );
    assert_eq!(
        cx.debug_bounds("DASHED_VERTICAL"),
        Some(bounds(0., 1., 1., 80.))
    );
}

struct Colored;

impl gpui::Render for Colored {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        v_flex()
            .w(px(200.))
            .child(Divider::horizontal().color(DividerColor::Border))
            .child(Divider::horizontal().color(DividerColor::BorderFaded))
            .child(Divider::horizontal().color(DividerColor::BorderVariant))
    }
}

#[gpui::test]
fn each_divider_color_paints_the_themes_color(cx: &mut gpui::TestAppContext) {
    cx.update(crate::init);
    let (_view, cx) = cx.add_window_view(|_, _| Colored);
    cx.update(|window, cx| {
        window.refresh();
        let _ = window.draw(cx);
        let quads = window.painted_quads();
        assert_eq!(
            quads.len(),
            3,
            "three solid dividers paint three quads: {quads:?}"
        );
        for (quad, color) in quads.iter().zip(COLORS) {
            assert_eq!(
                quad.background,
                themed(cx, color).into(),
                "{color:?} paints its theme color"
            );
        }
    });
}

struct Blocks;

impl gpui::Render for Blocks {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        h_flex()
            .child(
                div()
                    .h(px(80.))
                    .debug_below()
                    .debug_selector(|| "SOLID_BLOCK".into())
                    .child(Divider::vertical().inset()),
            )
            .child(
                div()
                    .h(px(80.))
                    .debug_below()
                    .debug_selector(|| "DASHED_BLOCK".into())
                    .child(Divider::vertical_dashed().inset()),
            )
    }
}

// A dashed divider's line is a path, not a quad. `debug_below` makes every
// element paint an outline at its own bounds, so the line's bounds are in
// `painted_quads` even then, while the divider's parent stays a plain block.
#[gpui::test]
fn an_inset_vertical_divider_in_a_plain_block_spans_its_height_less_a_margin_at_each_end(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::init);
    let (_view, cx) = cx.add_window_view(|_, _| Blocks);
    let (quads, factor) = cx.update(|window, cx| {
        window.refresh();
        let _ = window.draw(cx);
        (window.painted_quads(), window.scale_factor())
    });
    for (selector, name) in [("SOLID_BLOCK", "solid"), ("DASHED_BLOCK", "dashed")] {
        let block = cx.debug_bounds(selector).expect(selector).scale(factor);
        let line = quads
            .iter()
            .map(|quad| quad.bounds)
            .filter(|bounds| bounds.origin.x == block.origin.x)
            .filter(|bounds| bounds.size.height < block.size.height)
            .max_by_key(|bounds| bounds.size.height)
            .unwrap_or_else(|| panic!("the {name} divider paints no line inside its block"));
        assert_eq!(
            line.size.width,
            px(1.).scale(factor),
            "the {name} divider's line is one pixel wide"
        );
        assert!(
            line.size.height > block.size.height / 2. && line.size.height < block.size.height,
            "the {name} divider's line spans the block's height less a margin at each \
             end: {line:?} in {block:?}"
        );
    }
}
