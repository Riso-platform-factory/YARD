use super::{Modal, ModalFooter, ModalHeader, Section, SectionHeader};
use crate::{
    Assets, IconName, Label,
    dev::{Button, ButtonStyle, ElevationIndex, IconButton},
    v_flex,
};
use gpui::{AppContext, InteractiveElement, IntoElement, ParentElement, Styled, div, px, rems};
use std::{cell::Cell, rc::Rc, sync::Arc};

// A window whose app has the bundled assets, so each icon's file is really
// loaded when the close button paints.
fn draw_in_a_window_with_assets(view: impl gpui::Render + 'static) {
    let mut cx = gpui::HeadlessAppContext::with_asset_source(
        Arc::new(gpui::NoopTextSystem),
        Arc::new(Assets),
    );
    cx.update(crate::init);
    let window = cx
        .open_window(gpui::size(gpui::px(1024.), gpui::px(768.)), |_, cx| {
            cx.new(|_| view)
        })
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        window.refresh();
        let _ = window.draw(cx);
    })
    .unwrap();
}

// Draws a frame so `debug_bounds` sees the wrapped elements.
fn draw_a_frame(cx: &mut gpui::VisualTestContext) {
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

// Draws a frame so `debug_bounds` sees the wrapped element, then clicks its center.
fn click_center(cx: &mut gpui::VisualTestContext, selector: &'static str) {
    draw_a_frame(cx);
    let bounds = cx.debug_bounds(selector).expect(selector);
    cx.simulate_click(bounds.center(), gpui::Modifiers::default());
}

struct Modals;

impl gpui::Render for Modals {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        v_flex()
            .child(
                Modal::new("full")
                    .header(
                        ModalHeader::new()
                            .headline("Headline")
                            .description("A description")
                            .on_dismiss(|_, _| {}),
                    )
                    .section(
                        Section::new()
                            .header(SectionHeader::new("First section"))
                            .child(Label::new("first child")),
                    )
                    .section(
                        Section::new()
                            .header(SectionHeader::new("Second section"))
                            .child(Label::new("second child")),
                    )
                    .footer(
                        ModalFooter::new()
                            .start_slot(Button::new("cancel", "Cancel"))
                            .end_slot(Button::new("confirm", "Confirm")),
                    ),
            )
            .child(Modal::new("bare"))
    }
}

#[test]
fn a_modal_with_every_part_and_one_with_no_part_draw_in_a_window_with_assets() {
    draw_in_a_window_with_assets(Modals);
}

struct Dismissable {
    dismissals: Rc<Cell<usize>>,
}

impl gpui::Render for Dismissable {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let dismissals = self.dismissals.clone();
        v_flex().size_full().child(
            Modal::new("dismissable").header(
                ModalHeader::new()
                    .headline("Dismissable")
                    .on_dismiss(move |_, _| {
                        dismissals.set(dismissals.get() + 1);
                    }),
            ),
        )
    }
}

#[gpui::test]
fn a_click_on_the_close_button_calls_on_dismiss_once(cx: &mut gpui::TestAppContext) {
    cx.update(crate::init);
    let dismissals = Rc::new(Cell::new(0));
    let counted = dismissals.clone();
    let (_view, cx) = cx.add_window_view(move |_, _| Dismissable {
        dismissals: counted,
    });
    draw_a_frame(cx);
    let close = cx
        .debug_bounds("ICON-Close")
        .expect("a modal with on_dismiss draws a close button");
    cx.simulate_click(close.center(), gpui::Modifiers::default());
    assert_eq!(dismissals.get(), 1);
}

struct Undismissable;

impl gpui::Render for Undismissable {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(
                Modal::new("undismissable").header(
                    ModalHeader::new()
                        .headline("Headline")
                        .description("No close button"),
                ),
            )
            .child(Modal::new("bare"))
    }
}

#[gpui::test]
fn without_on_dismiss_there_is_no_close_button(cx: &mut gpui::TestAppContext) {
    cx.update(crate::init);
    let (_view, cx) = cx.add_window_view(|_, _| Undismissable);
    draw_a_frame(cx);
    assert!(
        cx.debug_bounds("ICON-Close").is_none(),
        "a modal without on_dismiss draws no close button"
    );
}

struct FooterButtons {
    start_clicks: Rc<Cell<usize>>,
    end_clicks: Rc<Cell<usize>>,
}

impl gpui::Render for FooterButtons {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let start_clicks = self.start_clicks.clone();
        let end_clicks = self.end_clicks.clone();
        v_flex().size_full().child(
            Modal::new("footered").footer(
                ModalFooter::new()
                    .start_slot(div().debug_selector(|| "START_BUTTON".into()).child(
                        Button::new("start", "Start").on_click(move |_, _| {
                            start_clicks.set(start_clicks.get() + 1);
                        }),
                    ))
                    .end_slot(div().debug_selector(|| "END_BUTTON".into()).child(
                        Button::new("end", "End").on_click(move |_, _| {
                            end_clicks.set(end_clicks.get() + 1);
                        }),
                    )),
            ),
        )
    }
}

#[gpui::test]
fn the_footers_slots_draw_start_left_of_end_and_each_click_reaches_its_own_counter(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::init);
    let start_clicks = Rc::new(Cell::new(0));
    let end_clicks = Rc::new(Cell::new(0));
    let start_counted = start_clicks.clone();
    let end_counted = end_clicks.clone();
    let (_view, cx) = cx.add_window_view(move |_, _| FooterButtons {
        start_clicks: start_counted,
        end_clicks: end_counted,
    });
    draw_a_frame(cx);
    let start = cx
        .debug_bounds("START_BUTTON")
        .expect("start button bounds");
    let end = cx.debug_bounds("END_BUTTON").expect("end button bounds");
    assert!(
        start.right() <= end.left(),
        "the start slot is left of the end slot: {start:?} vs {end:?}"
    );
    click_center(cx, "START_BUTTON");
    assert_eq!(start_clicks.get(), 1);
    assert_eq!(end_clicks.get(), 0);
    click_center(cx, "END_BUTTON");
    assert_eq!(end_clicks.get(), 1);
}

struct LayeredFooter {
    button_clicks: Rc<Cell<usize>>,
    icon_clicks: Rc<Cell<usize>>,
}

impl gpui::Render for LayeredFooter {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let button_clicks = self.button_clicks.clone();
        let icon_clicks = self.icon_clicks.clone();
        v_flex().size_full().child(
            Modal::new("layered").footer(
                ModalFooter::new()
                    .start_slot(
                        div().debug_selector(|| "LAYERED_BUTTON".into()).child(
                            Button::new("confirm", "Confirm")
                                .style(ButtonStyle::Filled)
                                .layer(ElevationIndex::ModalSurface)
                                .on_click(move |_, _| {
                                    button_clicks.set(button_clicks.get() + 1);
                                }),
                        ),
                    )
                    .end_slot(
                        div().debug_selector(|| "LAYERED_ICON".into()).child(
                            IconButton::new("modal-icon", IconName::Check)
                                .layer(ElevationIndex::ModalSurface)
                                .on_click(move |_, _| {
                                    icon_clicks.set(icon_clicks.get() + 1);
                                }),
                        ),
                    ),
            ),
        )
    }
}

#[gpui::test]
fn a_filled_button_and_an_icon_button_on_the_modal_surface_draw_and_each_click_reaches_its_own_counter(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::init);
    let button_clicks = Rc::new(Cell::new(0));
    let icon_clicks = Rc::new(Cell::new(0));
    let button_counted = button_clicks.clone();
    let icon_counted = icon_clicks.clone();
    let (_view, cx) = cx.add_window_view(move |_, _| LayeredFooter {
        button_clicks: button_counted,
        icon_clicks: icon_counted,
    });
    click_center(cx, "LAYERED_BUTTON");
    assert_eq!(button_clicks.get(), 1);
    assert_eq!(icon_clicks.get(), 0);
    click_center(cx, "LAYERED_ICON");
    assert_eq!(icon_clicks.get(), 1);
}

struct TwoWidths;

impl gpui::Render for TwoWidths {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        v_flex()
            .items_start()
            .child(
                div()
                    .debug_selector(|| "DEFAULT_WIDTH".into())
                    .child(Modal::new("default-width")),
            )
            .child(
                div()
                    .debug_selector(|| "GIVEN_WIDTH".into())
                    .child(Modal::new("given-width").width(rems(20.))),
            )
    }
}

#[gpui::test]
fn the_modal_is_thirty_rems_wide_unless_given_a_width(cx: &mut gpui::TestAppContext) {
    cx.update(crate::init);
    let (_view, cx) = cx.add_window_view(|_, _| TwoWidths);
    draw_a_frame(cx);
    let default = cx
        .debug_bounds("DEFAULT_WIDTH")
        .expect("the modal without a width");
    assert_eq!(
        default.size.width,
        px(480.),
        "30 rems at the default 16px rem"
    );
    let given = cx
        .debug_bounds("GIVEN_WIDTH")
        .expect("the modal with a width");
    assert_eq!(given.size.width, px(320.), "the given 20 rems");
}

struct Stacked;

impl gpui::Render for Stacked {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        v_flex().size_full().child(
            Modal::new("stacked")
                .header(ModalHeader::new().headline("Stacked").on_dismiss(|_, _| {}))
                .section(
                    Section::new()
                        .header(SectionHeader::new("First section"))
                        .child(
                            div()
                                .debug_selector(|| "FIRST_CHILD".into())
                                .child(Label::new("first child")),
                        ),
                )
                .section(
                    Section::new()
                        .header(SectionHeader::new("Second section"))
                        .child(
                            div()
                                .debug_selector(|| "SECOND_CHILD".into())
                                .child(Label::new("second child")),
                        ),
                )
                .footer(
                    ModalFooter::new().end_slot(
                        div()
                            .debug_selector(|| "FOOTER_BUTTON".into())
                            .child(Button::new("done", "Done")),
                    ),
                ),
        )
    }
}

#[gpui::test]
fn a_sections_child_is_below_the_close_button_and_a_footer_button_below_the_section_children(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::init);
    let (_view, cx) = cx.add_window_view(|_, _| Stacked);
    draw_a_frame(cx);
    let close = cx
        .debug_bounds("ICON-Close")
        .expect("the close button's bounds");
    let first = cx
        .debug_bounds("FIRST_CHILD")
        .expect("the first section's child");
    let second = cx
        .debug_bounds("SECOND_CHILD")
        .expect("the second section's child");
    let footer_button = cx.debug_bounds("FOOTER_BUTTON").expect("the footer button");
    assert!(
        first.top() >= close.bottom(),
        "a section child is below the header's close button: {first:?} vs {close:?}"
    );
    assert!(
        second.top() >= first.bottom(),
        "the sections draw in order: {second:?} vs {first:?}"
    );
    assert!(
        footer_button.top() >= second.bottom(),
        "a footer button is below the section children: {footer_button:?} vs {second:?}"
    );
}

struct SameLabel;

impl gpui::Render for SameLabel {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        v_flex()
            .items_start()
            .child(
                Modal::new("one-modal")
                    .section(
                        Section::new().header(SectionHeader::new("Same")).child(
                            div()
                                .debug_selector(|| "FIRST_SAME".into())
                                .child(Label::new("first same")),
                        ),
                    )
                    .section(
                        Section::new().header(SectionHeader::new("Same")).child(
                            div()
                                .debug_selector(|| "SECOND_SAME".into())
                                .child(Label::new("second same")),
                        ),
                    ),
            )
            .child(
                Modal::new("another-modal").section(
                    Section::new().header(SectionHeader::new("Same")).child(
                        div()
                            .debug_selector(|| "OTHER_SAME".into())
                            .child(Label::new("other same")),
                    ),
                ),
            )
            .child(
                Modal::new("third-modal").section(
                    Section::new().header(SectionHeader::new("Same")).child(
                        div()
                            .debug_selector(|| "THIRD_SAME".into())
                            .child(Label::new("third same")),
                    ),
                ),
            )
    }
}

#[gpui::test]
fn sections_named_by_place_draw_same_labelled_sections_in_one_modal_and_in_two_modals(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::init);
    let (_view, cx) = cx.add_window_view(|_, _| SameLabel);
    draw_a_frame(cx);
    let first = cx
        .debug_bounds("FIRST_SAME")
        .expect("the first Same section's child draws");
    let second = cx
        .debug_bounds("SECOND_SAME")
        .expect("the second Same section's child draws");
    let other = cx
        .debug_bounds("OTHER_SAME")
        .expect("the Same section's child in another-modal draws");
    let third = cx
        .debug_bounds("THIRD_SAME")
        .expect("the Same section's child in third-modal draws");
    assert!(
        second.top() >= first.bottom(),
        "two sections labelled Same in one modal are each their own element: {first:?} vs {second:?}"
    );
    assert!(
        third.top() >= other.bottom(),
        "two modals with a Same section each draw one below the other: {other:?} vs {third:?}"
    );
}
