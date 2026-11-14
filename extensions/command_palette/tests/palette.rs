#![cfg(feature = "dev")]

use command_palette::dev;
use gpui::{
    Action, AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, VisualTestContext,
};
use riso_shell::Extension;
use std::{cell::RefCell, rc::Rc};

/// The palette's key, per the contract: `cmd-shift-p` on macOS and
/// `ctrl-shift-p` elsewhere.
#[cfg(target_os = "macos")]
const TOGGLE_KEY: &str = "cmd-shift-p";
#[cfg(not(target_os = "macos"))]
const TOGGLE_KEY: &str = "ctrl-shift-p";

// The test card's own actions. The query `qe` matches all four, and the
// contract's ranking decides their order alone: `quest` and `quiet` match at
// the start of a word (`quest` with one gap, `quiet` with two), while
// `banquet` and `bequest` match inside a word with one gap each and so tie,
// ordered by name.
gpui::actions!(palette_test, [Banquet, Bequest, Quest, Quiet]);

/// The `(action, node)` pairs the card handlers record: `node` is what
/// `focused_node` reports while the handler runs.
type Runs = Rc<RefCell<Vec<(&'static str, Option<String>)>>>;

fn record(runs: &Runs, action: &'static str, window: &gpui::Window, cx: &gpui::App) {
    let node = riso_shell::focused_node(window, cx).map(|node| node.to_string());
    runs.borrow_mut().push((action, node));
}

struct Card {
    node: String,
    runs: Runs,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let mut card = gpui::div()
            .debug_selector(|| format!("CARD-{}", self.node))
            .size_full();
        // The element handling the card's actions tracks the node's own
        // handle: they are available only while this card has the focus.
        if let Some(handle) = riso_shell::focus_handle(cx, &self.node) {
            card = card
                .track_focus(&handle)
                .key_context("palette_test")
                .on_action({
                    let runs = self.runs.clone();
                    move |_: &Banquet, window, cx| record(&runs, "banquet", window, cx)
                })
                .on_action({
                    let runs = self.runs.clone();
                    move |_: &Bequest, window, cx| record(&runs, "bequest", window, cx)
                })
                .on_action({
                    let runs = self.runs.clone();
                    move |_: &Quest, window, cx| record(&runs, "quest", window, cx)
                })
                .on_action({
                    let runs = self.runs.clone();
                    move |_: &Quiet, window, cx| record(&runs, "quiet", window, cx)
                });
        }
        card
    }
}

fn draw(window: gpui::WindowHandle<riso_shell::Shell>, cx: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

/// The node the dev shell says has the focus, if any.
fn focused(visual: &mut VisualTestContext) -> Option<String> {
    visual.update(|window, cx| riso_shell::focused_node(window, cx).map(|node| node.to_string()))
}

/// Whether the dev shell's layer shows a modal.
fn has_modal(visual: &mut VisualTestContext) -> bool {
    visual.update(|window, cx| riso_shell::has_modal(window, cx))
}

/// Clicks the center of the element with `selector`.
fn click(visual: &mut VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} has bounds"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
}

/// The `&'static` selector of palette row `index`; `debug_bounds` wants
/// `&'static str`, so the string is leaked.
fn row(index: usize) -> &'static str {
    Box::leak(format!("PALETTE-ROW-{index}").into_boxed_str())
}

/// The number of palette rows drawn in the last frame.
fn rows(visual: &mut VisualTestContext) -> usize {
    let mut index = 0;
    while visual.debug_bounds(row(index)).is_some() {
        index += 1;
    }
    index
}

/// The commands the palette must list for the current focus, in display
/// order: every available action once, without `NoAction`, `Unbind`, or the
/// palette's `Toggle`, by readable name.
fn expected_commands(visual: &mut VisualTestContext) -> Vec<String> {
    visual.update(|window, cx| {
        let mut names: Vec<String> = window
            .available_actions(cx)
            .iter()
            .map(|action| action.name().to_string())
            .collect();
        names.retain(|name| {
            let name = name.as_str();
            name != dev::Toggle::name_for_type()
                && !name.ends_with("::NoAction")
                && !name.ends_with("::Unbind")
        });
        names.sort();
        names.dedup();
        names.sort_by_key(|name| dev::humanize_action_name(name));
        names
    })
}

/// The row index of `name` in `expected` order.
fn row_of(expected: &[String], name: &str) -> usize {
    expected
        .iter()
        .position(|candidate| candidate == name)
        .unwrap_or_else(|| panic!("{name} is listed"))
}

/// The runs recorded so far.
fn recorded(runs: &Runs) -> Vec<(&'static str, Option<String>)> {
    runs.borrow().clone()
}

/// Opens a window over a row of cards `a` and `b`, after running the
/// palette's `init` on its own extension, as the startup runs every
/// extension's.
fn open_cards_app(cx: &mut TestAppContext, runs: Runs) -> gpui::WindowHandle<riso_shell::Shell> {
    cx.update(riso_ui::init);
    cx.update(|cx| {
        // The startup binds the default keys in `app-init`, before extensions run.
        riso_shell::bind_default_keys(cx);
        riso_ui::bind_text_input_keys(cx);
        let palette = Extension::new(cx, "command_palette").unwrap();
        dev::init(cx, &palette);
        let test = Extension::new(cx, "palette_test").unwrap();
        test.register(cx, "palette_test.card", &[], move |node, _, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
                runs: runs.clone(),
            })
            .into()
        });
        test.place_root(cx, "root", "riso.row");
        for node in ["a", "b"] {
            test.place(cx, node, "palette_test.card", "root", "children");
        }
    });
    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), riso_shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(800.), gpui::px(600.)));
    draw(window, cx);
    window
}

/// Opens the palette, types `qe`, presses `keys` (a keystroke sequence, ""
/// for none), then `enter`, and checks the one command that ran and where.
fn enter_runs(
    window: gpui::WindowHandle<riso_shell::Shell>,
    cx: &mut TestAppContext,
    visual: &mut VisualTestContext,
    runs: &Runs,
    keys: &str,
    action: &'static str,
) {
    visual.simulate_keystrokes(TOGGLE_KEY);
    draw(window, cx);
    visual.simulate_input("qe");
    draw(window, cx);
    if !keys.is_empty() {
        visual.simulate_keystrokes(keys);
    }
    visual.simulate_keystrokes("enter");
    draw(window, cx);
    let runs_of_a = recorded(runs);
    let last = runs_of_a.last().expect("`enter` ran a command");
    assert_eq!(last.0, action, "`enter` ran `{action}`");
    assert_eq!(
        last.1.as_deref(),
        Some("a"),
        "`{action}` ran with card `a` focused"
    );
    assert!(!has_modal(visual), "`enter` closed the palette");
    assert_eq!(
        focused(visual).as_deref(),
        Some("a"),
        "the focus is back on the card"
    );
}

#[gpui::test]
fn the_key_opens_the_palette_on_the_input_and_again_closes_it(cx: &mut TestAppContext) {
    let window = open_cards_app(cx, Runs::default());
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-a");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    visual.simulate_keystrokes(TOGGLE_KEY);
    draw(window, cx);
    assert!(
        has_modal(&mut visual),
        "the palette is a modal of the window's modal layer"
    );
    assert_eq!(
        focused(&mut visual),
        None,
        "the palette holds the focus: no node is focused"
    );
    // The palette's input has the focus: typing reaches it and narrows the list.
    visual.simulate_input("qe");
    draw(window, cx);
    assert_eq!(
        rows(&mut visual),
        4,
        "typing into the palette's input narrowed the list to the four matches"
    );
    visual.simulate_keystrokes(TOGGLE_KEY);
    draw(window, cx);
    assert!(
        !has_modal(&mut visual),
        "the same key again closed the palette"
    );
    assert_eq!(
        focused(&mut visual).as_deref(),
        Some("a"),
        "the focus is back on the card"
    );
}

#[gpui::test]
fn the_list_holds_the_cards_and_the_shells_actions_by_readable_name(cx: &mut TestAppContext) {
    let runs = Runs::default();
    let window = open_cards_app(cx, runs.clone());
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-a");
    draw(window, cx);
    let expected = expected_commands(&mut visual);
    for name in [
        "palette_test::Banquet",
        "palette_test::Bequest",
        "palette_test::Quest",
        "palette_test::Quiet",
        "riso::FocusDown",
        "riso::FocusLeft",
        "riso::FocusRight",
        "riso::FocusUp",
        "riso::Hide",
        "riso::SelectNextTab",
        "riso::SelectPreviousTab",
        "riso::ToggleZoom",
    ] {
        assert!(
            expected.iter().any(|candidate| candidate == name),
            "{name} is available where the focus is"
        );
    }
    visual.simulate_keystrokes(TOGGLE_KEY);
    draw(window, cx);
    click(&mut visual, row(row_of(&expected, "palette_test::Banquet")));
    draw(window, cx);
    assert_eq!(
        recorded(&runs),
        [("banquet", Some(String::from("a")))],
        "the first row by name ran `palette_test::Banquet` on the focused card"
    );
    // The shell's own actions are listed at their place in the name order:
    // `riso: focus right` moves the focus from `a` to `b`.
    visual.simulate_keystrokes(TOGGLE_KEY);
    draw(window, cx);
    click(&mut visual, row(row_of(&expected, "riso::FocusRight")));
    draw(window, cx);
    assert_eq!(
        focused(&mut visual).as_deref(),
        Some("b"),
        "the listed `riso: focus right` ran where the focus was"
    );
    click(&mut visual, "CARD-a");
    draw(window, cx);
    // The palette's `Toggle` is not listed: `toggle` matches `riso: toggle
    // zoom` alone.
    visual.simulate_keystrokes(TOGGLE_KEY);
    draw(window, cx);
    visual.simulate_input("toggle");
    draw(window, cx);
    assert_eq!(
        rows(&mut visual),
        1,
        "`command_palette::Toggle` is not listed: `toggle` has one match"
    );
    visual.simulate_keystrokes("enter");
    draw(window, cx);
    assert!(!has_modal(&mut visual));
    assert_eq!(
        cx.update(|cx| riso_shell::zoomed(cx)).as_deref(),
        Some("a"),
        "the one `toggle` match ran `riso::ToggleZoom` on `a`"
    );
}

#[gpui::test]
fn typing_narrows_the_list_and_an_empty_match_shows_no_matches(cx: &mut TestAppContext) {
    let runs = Runs::default();
    let window = open_cards_app(cx, runs.clone());
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-a");
    draw(window, cx);
    visual.simulate_keystrokes(TOGGLE_KEY);
    draw(window, cx);
    visual.simulate_input("qe");
    draw(window, cx);
    assert_eq!(
        rows(&mut visual),
        4,
        "`qe` matches the four commands whose names hold it as a subsequence"
    );
    visual.simulate_input("q");
    draw(window, cx);
    assert_eq!(
        rows(&mut visual),
        0,
        "`qeq` matches nothing: the palette shows `No matches`"
    );
    assert!(
        has_modal(&mut visual),
        "the palette stays open, showing `No matches`"
    );
    visual.simulate_keystrokes("enter");
    draw(window, cx);
    assert!(recorded(&runs).is_empty(), "nothing is selected to run");
    assert!(
        has_modal(&mut visual),
        "enter on an empty match ran nothing: the palette stays open"
    );
    visual.simulate_keystrokes("backspace");
    draw(window, cx);
    assert_eq!(
        rows(&mut visual),
        4,
        "back to `qe`: the four matches return"
    );
    visual.simulate_keystrokes("escape");
    draw(window, cx);
    assert!(!has_modal(&mut visual));
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    assert!(recorded(&runs).is_empty(), "escape ran nothing");
}

#[gpui::test]
fn down_and_up_move_the_selection_wrapping_and_enter_runs_it(cx: &mut TestAppContext) {
    let runs = Runs::default();
    let window = open_cards_app(cx, runs.clone());
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-a");
    draw(window, cx);
    // `qe` ranks the four commands `quest`, `quiet`, `banquet`, `bequest`:
    // word-start matches come before inside-word matches, fewer gaps before
    // more, and equal ranks order by name.
    enter_runs(window, cx, &mut visual, &runs, "", "quest");
    enter_runs(window, cx, &mut visual, &runs, "down", "quiet");
    enter_runs(window, cx, &mut visual, &runs, "up", "bequest");
    enter_runs(window, cx, &mut visual, &runs, "down up", "quest");
    enter_runs(
        window,
        cx,
        &mut visual,
        &runs,
        "down down down down",
        "quest",
    );
    assert_eq!(
        recorded(&runs).len(),
        5,
        "each `enter` ran the selection exactly once"
    );
}

#[gpui::test]
fn a_click_on_a_row_runs_it_and_escape_runs_nothing(cx: &mut TestAppContext) {
    let runs = Runs::default();
    let window = open_cards_app(cx, runs.clone());
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-a");
    draw(window, cx);
    let expected = expected_commands(&mut visual);
    visual.simulate_keystrokes(TOGGLE_KEY);
    draw(window, cx);
    click(&mut visual, row(row_of(&expected, "palette_test::Bequest")));
    draw(window, cx);
    assert_eq!(
        recorded(&runs),
        [("bequest", Some(String::from("a")))],
        "the clicked row ran `palette_test::Bequest` once, with `a` focused"
    );
    assert!(!has_modal(&mut visual), "the click closed the palette");
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    // In the narrowed list, row 2 is `banquet`: after the two word-start
    // matches, the inside-word matches order by name.
    visual.simulate_keystrokes(TOGGLE_KEY);
    draw(window, cx);
    visual.simulate_input("qe");
    draw(window, cx);
    click(&mut visual, row(2));
    draw(window, cx);
    assert_eq!(
        recorded(&runs),
        [
            ("bequest", Some(String::from("a"))),
            ("banquet", Some(String::from("a"))),
        ],
        "row 2 of the `qe` matches ran `palette_test::Banquet`"
    );
    assert!(!has_modal(&mut visual));
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    visual.simulate_keystrokes(TOGGLE_KEY);
    draw(window, cx);
    visual.simulate_keystrokes("escape");
    draw(window, cx);
    assert!(!has_modal(&mut visual), "escape closed the palette");
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    assert_eq!(recorded(&runs).len(), 2, "escape ran nothing");
}
