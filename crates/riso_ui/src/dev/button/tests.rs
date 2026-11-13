use super::*;
use crate::{Assets, IconName, IconSize, TextColor, v_flex};
use gpui::{AppContext, InteractiveElement, IntoElement, ParentElement, Styled, div};
use std::{cell::Cell, rc::Rc, sync::Arc};

const STYLES: [ButtonStyle; 8] = [
    ButtonStyle::Filled,
    ButtonStyle::Tinted(TintColor::Accent),
    ButtonStyle::Tinted(TintColor::Error),
    ButtonStyle::Tinted(TintColor::Warning),
    ButtonStyle::Tinted(TintColor::Success),
    ButtonStyle::Outlined,
    ButtonStyle::Subtle,
    ButtonStyle::Transparent,
];
const SIZES: [ButtonSize; 5] = [
    ButtonSize::Large,
    ButtonSize::Medium,
    ButtonSize::Default,
    ButtonSize::Compact,
    ButtonSize::None,
];
const SHAPES: [IconButtonShape; 2] = [IconButtonShape::Square, IconButtonShape::Wide];

#[test]
fn the_defaults_are_subtle_accent_default_and_wide() {
    assert_eq!(ButtonStyle::default(), ButtonStyle::Subtle);
    assert_eq!(TintColor::default(), TintColor::Accent);
    assert_eq!(ButtonSize::default(), ButtonSize::Default);
    assert_eq!(IconButtonShape::default(), IconButtonShape::Wide);
}

#[test]
fn every_button_size_gives_the_contracts_rems() {
    assert_eq!(ButtonSize::Large.rems(), gpui::rems(2.));
    assert_eq!(ButtonSize::Medium.rems(), gpui::rems(1.75));
    assert_eq!(ButtonSize::Default.rems(), gpui::rems(1.375));
    assert_eq!(ButtonSize::Compact.rems(), gpui::rems(1.125));
    assert_eq!(ButtonSize::None.rems(), gpui::rems(1.));
}

// A window whose app has the bundled assets, so each icon's file is really
// loaded when an icon button paints.
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

struct Buttons;

impl gpui::Render for Buttons {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let mut children = Vec::new();
        let mut id = 0usize;
        for style in STYLES {
            for size in SIZES {
                for icons in 0..4 {
                    let button = Button::new(("button", id), "Label").style(style).size(size);
                    id += 1;
                    children.push(
                        match icons {
                            0 => button,
                            1 => button.start_icon(IconName::Check),
                            2 => button.end_icon(IconName::Plus),
                            _ => button.start_icon(IconName::Check).end_icon(IconName::Plus),
                        }
                        .into_any_element(),
                    );
                }
            }
            children.push(
                Button::new(("disabled", id), "Label")
                    .style(style)
                    .disabled(true)
                    .into_any_element(),
            );
            id += 1;
            children.push(
                Button::new(("toggled", id), "Label")
                    .toggle_state(true)
                    .selected_style(style)
                    .into_any_element(),
            );
            id += 1;
        }
        v_flex().children(children)
    }
}

#[test]
fn every_style_size_and_state_of_button_draws_in_a_window_with_assets() {
    draw_in_a_window_with_assets(Buttons);
}

struct IconButtons;

impl gpui::Render for IconButtons {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let mut children = Vec::new();
        let mut id = 0usize;
        for shape in SHAPES {
            for style in STYLES {
                for size in SIZES {
                    children.push(
                        IconButton::new(("icon_button", id), IconName::Check)
                            .shape(shape)
                            .style(style)
                            .size(size)
                            .into_any_element(),
                    );
                    id += 1;
                }
            }
        }
        children.push(
            IconButton::new("disabled", IconName::Warning)
                .disabled(true)
                .into_any_element(),
        );
        children.push(
            IconButton::new("toggled", IconName::Check)
                .toggle_state(true)
                .selected_style(ButtonStyle::Tinted(TintColor::Accent))
                .into_any_element(),
        );
        children.push(
            IconButton::new("sized", IconName::Info)
                .shape(IconButtonShape::Square)
                .icon_size(IconSize::XLarge)
                .icon_color(TextColor::Warning)
                .into_any_element(),
        );
        v_flex().children(children)
    }
}

#[test]
fn every_shape_style_and_state_of_icon_button_draws_in_a_window_with_assets() {
    draw_in_a_window_with_assets(IconButtons);
}

// Draws a frame so `debug_bounds` sees the wrapped element, then clicks its center.
fn click_center(cx: &mut gpui::VisualTestContext, selector: &'static str) {
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let bounds = cx.debug_bounds(selector).expect(selector);
    cx.simulate_click(bounds.center(), gpui::Modifiers::default());
}

struct Clicker {
    clicks: Rc<Cell<usize>>,
}

impl gpui::Render for Clicker {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .debug_selector(|| "CLICKER".into())
            .child(Button::new("clicker", "Click me").on_click(move |_, _| {
                clicks.set(clicks.get() + 1);
            }))
    }
}

#[gpui::test]
fn a_click_on_a_button_calls_on_click_once(cx: &mut gpui::TestAppContext) {
    cx.update(crate::init);
    let clicks = Rc::new(Cell::new(0));
    let counted = clicks.clone();
    let (_view, cx) = cx.add_window_view(move |_, _| Clicker { clicks: counted });
    click_center(cx, "CLICKER");
    assert_eq!(clicks.get(), 1);
}

struct IconClicker {
    clicks: Rc<Cell<usize>>,
}

impl gpui::Render for IconClicker {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .debug_selector(|| "ICON_CLICKER".into())
            .child(
                IconButton::new("icon_clicker", IconName::Check).on_click(move |_, _| {
                    clicks.set(clicks.get() + 1);
                }),
            )
    }
}

#[gpui::test]
fn a_click_on_an_icon_button_calls_on_click_once(cx: &mut gpui::TestAppContext) {
    cx.update(crate::init);
    let clicks = Rc::new(Cell::new(0));
    let counted = clicks.clone();
    let (_view, cx) = cx.add_window_view(move |_, _| IconClicker { clicks: counted });
    click_center(cx, "ICON_CLICKER");
    assert_eq!(clicks.get(), 1);
}

struct Disabled {
    clicks: Rc<Cell<usize>>,
}

impl gpui::Render for Disabled {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let button = self.clicks.clone();
        let icon = self.clicks.clone();
        v_flex()
            .size_full()
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .debug_selector(|| "DISABLED_BUTTON".into())
                    .child(
                        Button::new("disabled_button", "Nope")
                            .disabled(true)
                            .on_click(move |_, _| {
                                button.set(button.get() + 1);
                            }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .debug_selector(|| "DISABLED_ICON".into())
                    .child(
                        IconButton::new("disabled_icon", IconName::Check)
                            .disabled(true)
                            .on_click(move |_, _| {
                                icon.set(icon.get() + 1);
                            }),
                    ),
            )
    }
}

#[gpui::test]
fn a_click_on_a_disabled_button_calls_nothing(cx: &mut gpui::TestAppContext) {
    cx.update(crate::init);
    let clicks = Rc::new(Cell::new(0));
    let counted = clicks.clone();
    let (_view, cx) = cx.add_window_view(move |_, _| Disabled { clicks: counted });
    click_center(cx, "DISABLED_BUTTON");
    click_center(cx, "DISABLED_ICON");
    assert_eq!(clicks.get(), 0);
}

struct Toggle {
    on: bool,
    handle: gpui::WeakEntity<Toggle>,
}

impl gpui::Render for Toggle {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let handle = self.handle.clone();
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .debug_selector(|| "TOGGLE".into())
            .child(
                IconButton::new("toggle", IconName::Check)
                    .toggle_state(self.on)
                    .on_click(move |_, cx| {
                        handle
                            .update(cx, |this, cx| {
                                this.on = !this.on;
                                cx.notify();
                            })
                            .unwrap();
                    }),
            )
    }
}

#[gpui::test]
fn a_view_toggling_an_icon_button_sees_its_state_flip(cx: &mut gpui::TestAppContext) {
    cx.update(crate::init);
    let (view, cx) = cx.add_window_view(|_, cx| Toggle {
        on: false,
        handle: cx.weak_entity(),
    });
    click_center(cx, "TOGGLE");
    assert!(view.read_with(cx, |this, _| this.on));
    click_center(cx, "TOGGLE");
    assert!(!view.read_with(cx, |this, _| this.on));
}
