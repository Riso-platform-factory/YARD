use gpui::{
    InteractiveElement, IntoElement, ParentElement, Styled, TestAppContext, VisualTestContext, div,
    px, size,
};
use yard_ui::{Icon, IconName, IconSize, Tab, TextColor, h_flex, v_flex};

// `TAB-<id>` comes from the tab element; `Tab::icon` marks the icon it adds
// `ICON-<id>`, so `debug_bounds` can find it inside the tab.
struct Tabs;

impl gpui::Render for Tabs {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        v_flex().size_full().items_start().child(
            h_flex()
                .child(Tab::new("with", "Alpha").icon(IconName::Info))
                .child(Tab::new("without", "Alpha")),
        )
    }
}

struct Reference;

impl gpui::Render for Reference {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        v_flex().size_full().items_start().children([
            Tab::new("with", "Alpha")
                .icon(IconName::Info)
                .into_any_element(),
            // The contract's icon, in the same small muted style a tab uses.
            div()
                .debug_selector(|| "ICON-REF".into())
                .child(
                    Icon::new(IconName::Info)
                        .size(IconSize::Small)
                        .color(TextColor::Muted),
                )
                .into_any_element(),
        ])
    }
}

fn draw_a_frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

#[gpui::test]
fn an_icon_makes_a_tab_wider_than_the_same_tab_without_one(cx: &mut TestAppContext) {
    cx.update(yard_ui::init);
    let window = cx.open_window(size(px(640.), px(480.)), |_, _| Tabs);
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    draw_a_frame(&mut cx);
    let with = cx.debug_bounds("TAB-with").expect("the icon tab draws");
    let without = cx.debug_bounds("TAB-without").expect("the plain tab draws");
    assert!(
        with.size.width > without.size.width,
        "the icon adds width: {with:?} vs {without:?}"
    );
    assert!(
        cx.debug_bounds("ICON-without").is_none(),
        "a tab built without .icon draws no icon"
    );
}

#[gpui::test]
fn the_icon_is_drawn_before_the_label_and_is_small(cx: &mut TestAppContext) {
    cx.update(yard_ui::init);
    let window = cx.open_window(size(px(640.), px(480.)), |_, _| Reference);
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    draw_a_frame(&mut cx);
    let tab = cx.debug_bounds("TAB-with").expect("the icon tab draws");
    let icon = cx
        .debug_bounds("ICON-with")
        .expect("the tab draws its icon");
    assert!(
        tab.contains(&icon.center()),
        "the icon draws inside the tab: {icon:?} in {tab:?}"
    );
    assert!(
        icon.center().x < tab.center().x,
        "the icon sits in the tab's left half, before the label: {icon:?} in {tab:?}"
    );
    let reference = cx
        .debug_bounds("ICON-REF")
        .expect("the reference icon draws");
    assert_eq!(
        icon.size, reference.size,
        "the tab's icon is small: {icon:?} vs {reference:?}"
    );
}
