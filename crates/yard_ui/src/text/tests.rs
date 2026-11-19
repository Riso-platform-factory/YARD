use super::*;
use crate::{h_flex, v_flex};
use gpui::{IntoElement, ParentElement, Styled};
use std::{cell::RefCell, rc::Rc};
use theme::ActiveTheme;

const LABEL_SIZES: [LabelSize; 4] = [
    LabelSize::Large,
    LabelSize::Default,
    LabelSize::Small,
    LabelSize::XSmall,
];
const HEADLINE_SIZES: [HeadlineSize; 5] = [
    HeadlineSize::XSmall,
    HeadlineSize::Small,
    HeadlineSize::Medium,
    HeadlineSize::Large,
    HeadlineSize::XLarge,
];
const TEXT_COLORS: [TextColor; 8] = [
    TextColor::Default,
    TextColor::Muted,
    TextColor::Accent,
    TextColor::Disabled,
    TextColor::Placeholder,
    TextColor::Error,
    TextColor::Warning,
    TextColor::Success,
];

#[test]
fn label_sizes_give_the_contracts_rems() {
    assert_eq!(LabelSize::Large.rems(), gpui::rems(1.));
    assert_eq!(LabelSize::Default.rems(), gpui::rems(0.875));
    assert_eq!(LabelSize::Small.rems(), gpui::rems(0.75));
    assert_eq!(LabelSize::XSmall.rems(), gpui::rems(0.625));
}

#[test]
fn headline_sizes_give_the_contracts_rems() {
    assert_eq!(HeadlineSize::XSmall.rems(), gpui::rems(0.88));
    assert_eq!(HeadlineSize::Small.rems(), gpui::rems(1.));
    assert_eq!(HeadlineSize::Medium.rems(), gpui::rems(1.125));
    assert_eq!(HeadlineSize::Large.rems(), gpui::rems(1.27));
    assert_eq!(HeadlineSize::XLarge.rems(), gpui::rems(1.43));
}

#[test]
fn the_defaults_are_label_default_headline_medium_and_color_default() {
    assert_eq!(LabelSize::default(), LabelSize::Default);
    assert_eq!(HeadlineSize::default(), HeadlineSize::Medium);
    assert_eq!(TextColor::default(), TextColor::Default);
}

// A TextColor's color, mapped as the contract says: five of the theme's text
// colors and three of its status colors.
fn themed(cx: &gpui::App, color: TextColor) -> gpui::Hsla {
    match color {
        TextColor::Default => cx.theme().colors().text,
        TextColor::Muted => cx.theme().colors().text_muted,
        TextColor::Accent => cx.theme().colors().text_accent,
        TextColor::Disabled => cx.theme().colors().text_disabled,
        TextColor::Placeholder => cx.theme().colors().text_placeholder,
        TextColor::Error => cx.theme().status().error,
        TextColor::Warning => cx.theme().status().warning,
        TextColor::Success => cx.theme().status().success,
    }
}

fn assert_colors_are_the_themes(cx: &gpui::App, theme: &str) {
    assert_eq!(crate::active_theme(cx), theme);
    for color in TEXT_COLORS {
        assert_eq!(color.hsla(cx), themed(cx, color), "{color:?} under {theme}");
    }
}

#[gpui::test]
fn every_text_color_is_the_themes_under_one_dark_and_one_light(cx: &mut gpui::TestAppContext) {
    cx.update(crate::init);
    cx.read(|cx| assert_colors_are_the_themes(cx, "One Dark"));
    cx.update(|cx| {
        let theme = theme::ThemeRegistry::global(cx).get("One Light").unwrap();
        theme::GlobalTheme::update_theme(cx, theme);
    });
    cx.read(|cx| assert_colors_are_the_themes(cx, "One Light"));
}

fn draw<V: 'static + gpui::Render>(
    cx: &mut gpui::TestAppContext,
    view: impl FnOnce(&mut gpui::Window, &mut gpui::Context<V>) -> V,
) {
    let window = cx.add_window(view);
    gpui::AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

struct Text;

impl gpui::Render for Text {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let mut children = Vec::new();
        for size in LABEL_SIZES {
            for color in TEXT_COLORS {
                children.push(
                    Label::new("label")
                        .size(size)
                        .color(color)
                        .into_any_element(),
                );
            }
        }
        for size in HEADLINE_SIZES {
            children.push(Headline::new("headline").size(size).into_any_element());
        }
        v_flex().children(children)
    }
}

#[gpui::test]
fn every_size_and_color_of_text_draws_in_a_window(cx: &mut gpui::TestAppContext) {
    cx.update(crate::init);
    draw(cx, |_, _| Text);
}

struct Stacks {
    bounds: Rc<RefCell<Vec<gpui::Bounds<gpui::Pixels>>>>,
}

impl gpui::Render for Stacks {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        self.bounds.borrow_mut().clear();
        let bounds = self.bounds.clone();
        let probe = |width: f32, height: f32| {
            let captured = bounds.clone();
            gpui::canvas(
                move |bounds, _, _| captured.borrow_mut().push(bounds),
                |_, _, _, _| {},
            )
            .w(gpui::px(width))
            .h(gpui::px(height))
            .into_any_element()
        };
        v_flex()
            .child(h_flex().child(probe(40., 20.)).child(probe(40., 40.)))
            .child(v_flex().child(probe(50., 20.)).child(probe(50., 30.)))
    }
}

#[gpui::test]
fn stacks_hold_children_as_a_centered_row_and_a_column(cx: &mut gpui::TestAppContext) {
    cx.update(crate::init);
    let bounds = Rc::new(RefCell::new(Vec::new()));
    let captured = bounds.clone();
    draw(cx, move |_, _| Stacks { bounds: captured });
    let bounds = bounds.borrow();
    assert_eq!(bounds.len(), 4);
    // In the row the 20px child sits 10px below the 40px one: centered across.
    assert_eq!(bounds[1].origin.x - bounds[0].origin.x, gpui::px(40.));
    assert_eq!(bounds[0].origin.y - bounds[1].origin.y, gpui::px(10.));
    // In the column the second child sits under the first.
    assert_eq!(bounds[3].origin.x - bounds[2].origin.x, gpui::px(0.));
    assert_eq!(bounds[3].origin.y - bounds[2].origin.y, gpui::px(20.));
}
