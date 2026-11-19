use gpui::{
    IntoElement, ParentElement, Styled, TestAppContext, VisualTestContext, point, px, size,
};
use std::{cell::Cell, rc::Rc};
use yard_ui::{Tab, h_flex, v_flex};

// `TAB-<id>` comes from the tab element; `Tab::on_close` marks the close
// button it adds `CLOSE-<id>`, as `Tab::icon` marks `ICON-<id>`, so
// `debug_bounds` can find it inside the tab.
type Calls = Rc<Cell<usize>>;

struct Tabs {
    closable: Calls,
    selected: Calls,
}

impl gpui::Render for Tabs {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let closable = self.closable.clone();
        let selected = self.selected.clone();
        v_flex().size_full().items_start().child(
            h_flex()
                .child(Tab::new("plain", "Plain"))
                .child(
                    Tab::new("closable", "Closable")
                        .on_close(move |_, _| closable.set(closable.get() + 1)),
                )
                .child(
                    Tab::new("selected", "Selected")
                        .selected(true)
                        .on_close(move |_, _| selected.set(selected.get() + 1)),
                ),
        )
    }
}

fn draw_a_frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn open(cx: &mut TestAppContext, tabs: Tabs) -> VisualTestContext {
    cx.update(yard_ui::init);
    let window = cx.open_window(size(px(640.), px(480.)), |_, _| tabs);
    VisualTestContext::from_window(window.into(), cx)
}

#[gpui::test]
fn a_tab_with_on_close_draws_its_button_in_the_end_slot(cx: &mut TestAppContext) {
    let mut cx = open(
        cx,
        Tabs {
            closable: Calls::default(),
            selected: Calls::default(),
        },
    );
    draw_a_frame(&mut cx);
    let tab = cx.debug_bounds("TAB-closable").expect("the tab draws");
    let button = cx
        .debug_bounds("CLOSE-closable")
        .expect("the close button's bounds are recorded");
    assert!(
        tab.contains(&button.center()),
        "the button draws inside the tab: {button:?} in {tab:?}"
    );
    assert!(
        button.center().x > tab.center().x,
        "the button sits at the tab's right end: {button:?} in {tab:?}"
    );
    assert!(
        cx.debug_bounds("CLOSE-plain").is_none(),
        "a tab without on_close draws no button"
    );
}

// The button's bounds are recorded whether it is drawn or not, so the check is
// a click: while hidden the click passes through to the tab and `f` is not
// called; while drawn it reaches the button.
#[gpui::test]
fn the_button_is_hidden_until_the_pointer_is_over_the_tab_also_on_the_selected_tab(
    cx: &mut TestAppContext,
) {
    let closable = Calls::default();
    let selected = Calls::default();
    let mut cx = open(
        cx,
        Tabs {
            closable: closable.clone(),
            selected: selected.clone(),
        },
    );
    draw_a_frame(&mut cx);
    let tab = cx
        .debug_bounds("TAB-closable")
        .expect("the tab draws")
        .center();
    let button = cx
        .debug_bounds("CLOSE-closable")
        .expect("the close button's bounds are recorded")
        .center();
    // The pointer rests in the window's corner, away from every tab.
    let corner = point(px(600.), px(440.));
    cx.simulate_mouse_move(corner, None, gpui::Modifiers::default());
    draw_a_frame(&mut cx);
    cx.simulate_click(button, gpui::Modifiers::default());
    assert_eq!(
        closable.get(),
        0,
        "hidden until the pointer is over the tab"
    );
    cx.simulate_mouse_move(tab, None, gpui::Modifiers::default());
    draw_a_frame(&mut cx);
    cx.simulate_click(button, gpui::Modifiers::default());
    assert_eq!(closable.get(), 1, "drawn while the pointer is over the tab");
    // On the selected tab too the button is hidden until the pointer is over
    // the tab.
    cx.simulate_mouse_move(corner, None, gpui::Modifiers::default());
    draw_a_frame(&mut cx);
    let selected_tab = cx
        .debug_bounds("TAB-selected")
        .expect("the selected tab draws")
        .center();
    let selected_button = cx
        .debug_bounds("CLOSE-selected")
        .expect("the selected tab's close button's bounds are recorded")
        .center();
    cx.simulate_click(selected_button, gpui::Modifiers::default());
    assert_eq!(selected.get(), 0, "hidden on the selected tab too");
    cx.simulate_mouse_move(selected_tab, None, gpui::Modifiers::default());
    draw_a_frame(&mut cx);
    cx.simulate_click(selected_button, gpui::Modifiers::default());
    assert_eq!(selected.get(), 1, "drawn while the pointer is over the tab");
}

#[gpui::test]
fn clicking_the_close_button_calls_on_close_once(cx: &mut TestAppContext) {
    let closable = Calls::default();
    let mut cx = open(
        cx,
        Tabs {
            closable: closable.clone(),
            selected: Calls::default(),
        },
    );
    draw_a_frame(&mut cx);
    let tab = cx
        .debug_bounds("TAB-closable")
        .expect("the tab draws")
        .center();
    cx.simulate_mouse_move(tab, None, gpui::Modifiers::default());
    draw_a_frame(&mut cx);
    let button = cx
        .debug_bounds("CLOSE-closable")
        .expect("the tab's close button draws on hover")
        .center();
    cx.simulate_click(button, gpui::Modifiers::default());
    assert_eq!(closable.get(), 1, "one click of the button calls f once");
}

#[gpui::test]
fn a_middle_click_on_the_tab_calls_on_close_once(cx: &mut TestAppContext) {
    let closable = Calls::default();
    let mut cx = open(
        cx,
        Tabs {
            closable: closable.clone(),
            selected: Calls::default(),
        },
    );
    draw_a_frame(&mut cx);
    let tab = cx
        .debug_bounds("TAB-closable")
        .expect("the tab draws")
        .center();
    cx.simulate_mouse_down(tab, gpui::MouseButton::Middle, gpui::Modifiers::default());
    cx.simulate_mouse_up(tab, gpui::MouseButton::Middle, gpui::Modifiers::default());
    assert_eq!(closable.get(), 1, "one middle click calls f once");
}

#[gpui::test]
fn a_tab_without_on_close_has_no_button_and_ignores_a_middle_click(cx: &mut TestAppContext) {
    let closable = Calls::default();
    let selected = Calls::default();
    let mut cx = open(
        cx,
        Tabs {
            closable: closable.clone(),
            selected: selected.clone(),
        },
    );
    draw_a_frame(&mut cx);
    assert!(
        cx.debug_bounds("CLOSE-plain").is_none(),
        "a tab without on_close has no close button"
    );
    let tab = cx
        .debug_bounds("TAB-plain")
        .expect("the tab draws")
        .center();
    cx.simulate_mouse_down(tab, gpui::MouseButton::Middle, gpui::Modifiers::default());
    cx.simulate_mouse_up(tab, gpui::MouseButton::Middle, gpui::Modifiers::default());
    assert_eq!(
        (closable.get(), selected.get()),
        (0, 0),
        "the middle click closes nothing"
    );
}
