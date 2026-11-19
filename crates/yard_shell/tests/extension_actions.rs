#![cfg(target_os = "macos")]

use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

gpui::actions!(test_ext, [Increment, Ping]);

type Counts = Rc<RefCell<BTreeMap<String, usize>>>;

struct Card {
    node: String,
    counts: Counts,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let selector = self.node.clone();
        let node = self.node.clone();
        let counts = self.counts.clone();
        let mut card = gpui::div()
            .debug_selector(move || format!("CARD-{selector}"))
            .size_full();
        // The element that handles the component's action tracks the node's own
        // handle: the action runs only while this card has the focus.
        if let Some(handle) = yard_shell::focus_handle(cx, &self.node) {
            card = card.track_focus(&handle).key_context("test_ext").on_action(
                move |_: &Increment, _, _| {
                    *counts.borrow_mut().entry(node.clone()).or_default() += 1;
                },
            );
        }
        card
    }
}

fn draw(window: gpui::WindowHandle<yard_shell::Shell>, cx: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

/// Binds the shell's default keys, registers `test_ext.card`, and opens a
/// window over a row of cards `a` and `b`. `init` binds the extension's keys
/// and registers its window-wide handlers.
fn open_cards_app(
    cx: &mut TestAppContext,
    counts: Counts,
    init: impl FnOnce(&mut gpui::App, &yard_shell::Extension),
) -> gpui::WindowHandle<yard_shell::Shell> {
    cx.update(|cx| {
        // The startup binds the default keys in `app-init`, before extensions run.
        yard_shell::bind_default_keys(cx);
        let ext = yard_shell::Extension::new(cx, "test_ext").unwrap();
        ext.register(cx, "test_ext.card", &[], move |node, _, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
                counts: counts.clone(),
            })
            .into()
        });
        init(cx, &ext);
        ext.place_root(cx, "root", "yard.row");
        for node in ["a", "b"] {
            ext.place(cx, node, "test_ext.card", "root", "children");
        }
    });
    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(640.), gpui::px(480.)));
    draw(window, cx);
    window
}

/// The node the dev shell says has the focus, if any.
fn focused(visual: &mut gpui::VisualTestContext) -> Option<String> {
    visual.update(|window, cx| yard_shell::focused_node(window, cx).map(|node| node.to_string()))
}

/// Clicks the center of the element with `selector`.
fn click(visual: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} has bounds"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
}

#[gpui::test]
fn ctrl_alt_i_counts_on_the_focused_card_and_does_nothing_without_focus(cx: &mut TestAppContext) {
    cx.update(yard_ui::init);
    let counts = Rc::new(RefCell::new(BTreeMap::new()));
    let window = open_cards_app(cx, counts.clone(), |cx, ext| {
        ext.bind_keys(cx, &[("ctrl-alt-i", Box::new(Increment), "test_ext")]);
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    // Before anything is clicked, no card has the focus: the key does nothing.
    assert_eq!(focused(&mut visual), None);
    visual.simulate_keystrokes("ctrl-alt-i");
    draw(window, cx);
    assert!(counts.borrow().is_empty());
    click(&mut visual, "CARD-a");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    visual.simulate_keystrokes("ctrl-alt-i");
    visual.simulate_keystrokes("ctrl-alt-i");
    draw(window, cx);
    assert_eq!(counts.borrow().get("a"), Some(&2));
    assert_eq!(counts.borrow().get("b"), None);
    click(&mut visual, "CARD-b");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
    visual.simulate_keystrokes("ctrl-alt-i");
    draw(window, cx);
    assert_eq!(counts.borrow().get("a"), Some(&2));
    assert_eq!(counts.borrow().get("b"), Some(&1));
}

#[gpui::test]
fn ctrl_alt_p_pings_wherever_the_focus_is(cx: &mut TestAppContext) {
    cx.update(yard_ui::init);
    let counts = Rc::new(RefCell::new(BTreeMap::new()));
    let pings = Rc::new(Cell::new(0usize));
    let handler_pings = pings.clone();
    let window = open_cards_app(cx, counts, |cx, ext| {
        ext.bind_keys(cx, &[("ctrl-alt-p", Box::new(Ping), "Shell")]);
        ext.on_action::<Ping>(cx, move |_, _, _| {
            handler_pings.set(handler_pings.get() + 1)
        });
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    // No card has the focus: the window-wide handler runs anyway.
    assert_eq!(focused(&mut visual), None);
    visual.simulate_keystrokes("ctrl-alt-p");
    draw(window, cx);
    assert_eq!(pings.get(), 1);
    for card in ["CARD-a", "CARD-b"] {
        click(&mut visual, card);
        draw(window, cx);
        visual.simulate_keystrokes("ctrl-alt-p");
        draw(window, cx);
    }
    assert_eq!(pings.get(), 3);
}

#[gpui::test]
fn an_unreadable_binding_is_refused_and_the_others_are_bound(cx: &mut TestAppContext) {
    cx.update(yard_ui::init);
    let counts = Rc::new(RefCell::new(BTreeMap::new()));
    let pings = Rc::new(Cell::new(0usize));
    let handler_pings = pings.clone();
    let window = open_cards_app(cx, counts.clone(), |cx, ext| {
        ext.bind_keys(
            cx,
            &[
                ("ctrl-alt-i", Box::new(Increment), "test_ext"),
                ("ctrl-alt-", Box::new(Ping), "Shell"),
                ("ctrl-alt-p", Box::new(Ping), "Shell"),
            ],
        );
        ext.on_action::<Ping>(cx, move |_, _, _| {
            handler_pings.set(handler_pings.get() + 1)
        });
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-a");
    draw(window, cx);
    visual.simulate_keystrokes("ctrl-alt-i");
    visual.simulate_keystrokes("ctrl-alt-p");
    draw(window, cx);
    // The bindings on each side of `ctrl-alt-` work, and nothing panicked.
    assert_eq!(counts.borrow().get("a"), Some(&1));
    assert_eq!(pings.get(), 1);
}

#[gpui::test]
fn handling_a_shell_action_is_refused_and_it_still_hides_the_focused_card(cx: &mut TestAppContext) {
    cx.update(yard_ui::init);
    let counts = Rc::new(RefCell::new(BTreeMap::new()));
    let window = open_cards_app(cx, counts, |cx, ext| {
        ext.on_action::<yard_shell::Hide>(cx, |_, _, _| {
            panic!("test_ext cannot handle yard::Hide")
        });
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-a");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    visual.dispatch_action(yard_shell::Hide);
    draw(window, cx);
    // `Hide` stays the shell's action: it hides the focused card as before.
    assert!(cx.update(|cx| yard_shell::is_hidden(cx, "a")));
    assert!(!cx.update(|cx| yard_shell::is_hidden(cx, "b")));
    assert_eq!(focused(&mut visual), None);
}

#[gpui::test]
fn an_extension_key_in_shell_overrides_the_shells_default_key(cx: &mut TestAppContext) {
    cx.update(yard_ui::init);
    let counts = Rc::new(RefCell::new(BTreeMap::new()));
    let pings = Rc::new(Cell::new(0usize));
    let handler_pings = pings.clone();
    let window = open_cards_app(cx, counts, |cx, ext| {
        // `bind_default_keys` bound `shift-escape` to `ToggleZoom` in `Shell`
        // first; the extension's later binding in the same context wins.
        ext.bind_keys(cx, &[("shift-escape", Box::new(Ping), "Shell")]);
        ext.on_action::<Ping>(cx, move |_, _, _| {
            handler_pings.set(handler_pings.get() + 1)
        });
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-a");
    draw(window, cx);
    visual.simulate_keystrokes("shift-escape");
    draw(window, cx);
    assert_eq!(pings.get(), 1);
    assert_eq!(cx.update(|cx| yard_shell::zoomed(cx)), None);
}
