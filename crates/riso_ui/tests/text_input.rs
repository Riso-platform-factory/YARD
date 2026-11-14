#![cfg(feature = "dev")]

use gpui::{
    AppContext, Bounds, ClipboardItem, Entity, EntityInputHandler, Focusable, InteractiveElement,
    KeyBinding, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, ParentElement, Pixels, Point,
    Styled, TestAppContext, VisualTestContext, div, point, px, size,
};
use riso_ui::{
    dev::{TextInput, TextInputEvent, bind_text_input_keys},
    v_flex,
};
use std::{
    cell::{Cell, RefCell},
    ops::Range,
    rc::Rc,
};

// A test action an ancestor of the input binds to `escape`: the contract says
// the input does not bind it, so it must reach the ancestor.
gpui::actions!(text_input_tests, [Escaped]);

// The contract's key table, once per OS.
#[cfg(target_os = "macos")]
mod keys {
    pub const WORD_BACKSPACE: &str = "alt-backspace";
    pub const WORD_LEFT: &str = "alt-left";
    pub const WORD_RIGHT: &str = "alt-right";
    pub const LINE_START: &str = "cmd-left";
    pub const LINE_END: &str = "cmd-right";
    pub const SELECT_WORD_LEFT: &str = "alt-shift-left";
    pub const SELECT_WORD_RIGHT: &str = "alt-shift-right";
    pub const SELECT_LINE_START: &str = "cmd-shift-left";
    pub const SELECT_LINE_END: &str = "cmd-shift-right";
    pub const SELECT_ALL: &str = "cmd-a";
    pub const COPY: &str = "cmd-c";
    pub const CUT: &str = "cmd-x";
    pub const PASTE: &str = "cmd-v";
    pub const UNDO: &str = "cmd-z";
    pub const REDO: &str = "cmd-shift-z";
}

#[cfg(not(target_os = "macos"))]
mod keys {
    pub const WORD_BACKSPACE: &str = "ctrl-backspace";
    pub const WORD_LEFT: &str = "ctrl-left";
    pub const WORD_RIGHT: &str = "ctrl-right";
    pub const LINE_START: &str = "home";
    pub const LINE_END: &str = "end";
    pub const SELECT_WORD_LEFT: &str = "ctrl-shift-left";
    pub const SELECT_WORD_RIGHT: &str = "ctrl-shift-right";
    pub const SELECT_LINE_START: &str = "shift-home";
    pub const SELECT_LINE_END: &str = "shift-end";
    pub const SELECT_ALL: &str = "ctrl-a";
    pub const COPY: &str = "ctrl-c";
    pub const CUT: &str = "ctrl-x";
    pub const PASTE: &str = "ctrl-v";
    pub const UNDO: &str = "ctrl-z";
    pub const REDO: &str = "ctrl-shift-z";
}

struct Host {
    input: Entity<TextInput>,
    width: Pixels,
    escaped: Rc<Cell<usize>>,
}

impl gpui::Render for Host {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        let escaped = self.escaped.clone();
        v_flex()
            .key_context("Probe")
            .on_action(move |_: &Escaped, _, _| escaped.set(escaped.get() + 1))
            .size_full()
            .child(
                div()
                    .debug_selector(|| "INPUT".into())
                    .w(self.width)
                    .child(self.input.clone()),
            )
    }
}

struct Harness {
    input: Entity<TextInput>,
    events: Rc<RefCell<Vec<&'static str>>>,
    escaped: Rc<Cell<usize>>,
}

impl Harness {
    fn text(&self, cx: &mut VisualTestContext) -> String {
        self.input.read_with(cx, |input, _| input.text().to_owned())
    }

    // The selection, in UTF-16 offsets; equal to byte offsets for ASCII text.
    fn selection(&self, cx: &mut VisualTestContext) -> Range<usize> {
        cx.update(|window, cx| {
            self.input.update(cx, |input, cx| {
                input
                    .selected_text_range(false, window, cx)
                    .expect("the input reports its selection")
                    .range
            })
        })
    }

    // The bounds the input reports for a UTF-16 range, with `element` standing
    // in for the bounds of the painted text element.
    fn bounds_of(
        &self,
        cx: &mut VisualTestContext,
        element: Bounds<Pixels>,
        range: Range<usize>,
    ) -> Bounds<Pixels> {
        cx.update(|window, cx| {
            self.input.update(cx, |input, cx| {
                input.bounds_for_range(range, element, window, cx)
            })
        })
        .expect("the input reports the bounds of a text range")
    }

    fn index_at(&self, cx: &mut VisualTestContext, point: Point<Pixels>) -> usize {
        cx.update(|window, cx| {
            self.input.update(cx, |input, cx| {
                input.character_index_for_point(point, window, cx)
            })
        })
        .expect("the input maps a point inside it to a text index")
    }

    fn changed(&self) -> usize {
        self.events
            .borrow()
            .iter()
            .filter(|event| **event == "changed")
            .count()
    }

    fn confirmed(&self) -> usize {
        self.events
            .borrow()
            .iter()
            .filter(|event| **event == "confirmed")
            .count()
    }
}

// Draws a frame so `debug_bounds` sees the wrapped elements.
fn draw_a_frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

// Opens a 400x600 window holding an input that is `width` wide, subscribes to
// its events, and focuses it unless `focus` is false.
fn open(cx: &mut TestAppContext, width: f32, focus: bool) -> (Harness, VisualTestContext) {
    cx.update(|cx| {
        riso_ui::init(cx);
        bind_text_input_keys(cx);
        cx.bind_keys([KeyBinding::new("escape", Escaped, Some("Probe"))]);
    });
    let escaped = Rc::new(Cell::new(0));
    let window = cx.open_window(size(px(400.), px(600.)), {
        let escaped = escaped.clone();
        move |window, cx| Host {
            input: cx.new(|cx| TextInput::new(window, cx).placeholder("Search…")),
            width: px(width),
            escaped,
        }
    });
    let input = window
        .root(cx)
        .expect("the window has a root view")
        .read_with(cx, |host, _| host.input.clone());
    let events = Rc::new(RefCell::new(Vec::new()));
    let seen = events.clone();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.update(|window, cx| {
        cx.subscribe(&input, move |_, event: &TextInputEvent, _| {
            seen.borrow_mut().push(match event {
                TextInputEvent::Changed => "changed",
                TextInputEvent::Confirmed => "confirmed",
            });
        })
        .detach();
        if focus {
            input.focus_handle(cx).focus(window, cx);
        }
    });
    cx.run_until_parked();
    (
        Harness {
            input,
            events,
            escaped,
        },
        cx,
    )
}

#[gpui::test]
fn typing_backspace_delete_and_word_backspace_change_the_text_once_each(cx: &mut TestAppContext) {
    let (input, mut cx) = open(cx, 240., true);
    cx.simulate_input("héllo");
    assert_eq!(input.text(&mut cx), "héllo");
    assert_eq!(input.changed(), 5, "one Changed per character");
    cx.simulate_keystrokes("backspace");
    assert_eq!(input.text(&mut cx), "héll");
    cx.simulate_keystrokes(keys::WORD_BACKSPACE);
    assert_eq!(
        input.text(&mut cx),
        "",
        "the word key deletes the word before the cursor"
    );
    cx.simulate_input("ab");
    cx.simulate_keystrokes("left");
    assert_eq!(input.changed(), 9, "a move is not a change");
    cx.simulate_keystrokes("delete");
    assert_eq!(input.text(&mut cx), "a");
    assert_eq!(input.changed(), 10);
    assert_eq!(input.confirmed(), 0);
}

#[gpui::test]
fn moving_and_selecting_with_the_bound_keys_and_typing_over_the_selection(cx: &mut TestAppContext) {
    let (input, mut cx) = open(cx, 240., true);
    cx.simulate_input("one two");
    cx.simulate_keystrokes("left");
    assert_eq!(input.selection(&mut cx), 6..6);
    cx.simulate_keystrokes("right");
    assert_eq!(input.selection(&mut cx), 7..7);
    cx.simulate_keystrokes(keys::WORD_LEFT);
    assert_eq!(input.selection(&mut cx), 4..4);
    cx.simulate_keystrokes(keys::WORD_LEFT);
    assert_eq!(input.selection(&mut cx), 0..0);
    cx.simulate_keystrokes(keys::WORD_RIGHT);
    assert_eq!(input.selection(&mut cx), 3..3);
    cx.simulate_keystrokes(keys::LINE_END);
    assert_eq!(input.selection(&mut cx), 7..7);
    cx.simulate_keystrokes(keys::LINE_START);
    assert_eq!(input.selection(&mut cx), 0..0);
    assert_eq!(input.changed(), 7, "a move is not a change");
    cx.simulate_keystrokes("shift-right");
    assert_eq!(input.selection(&mut cx), 0..1);
    cx.simulate_keystrokes(keys::SELECT_WORD_RIGHT);
    assert_eq!(input.selection(&mut cx), 0..3);
    cx.simulate_keystrokes(keys::SELECT_LINE_END);
    assert_eq!(input.selection(&mut cx), 0..7);
    cx.simulate_keystrokes("left");
    assert_eq!(
        input.selection(&mut cx),
        0..0,
        "leaving a selection sideways lands on its edge"
    );
    cx.simulate_keystrokes(keys::LINE_END);
    cx.simulate_keystrokes(keys::SELECT_WORD_LEFT);
    assert_eq!(input.selection(&mut cx), 4..7);
    cx.simulate_keystrokes(keys::SELECT_LINE_START);
    assert_eq!(input.selection(&mut cx), 0..7);
    assert_eq!(input.changed(), 7, "a selection is not a change");
    cx.simulate_input("x");
    assert_eq!(input.text(&mut cx), "x", "typing replaces the selection");
    assert_eq!(input.selection(&mut cx), 1..1);
    cx.simulate_input("abc");
    cx.simulate_keystrokes(keys::SELECT_ALL);
    assert_eq!(input.selection(&mut cx), 0..4);
    cx.simulate_input("done");
    assert_eq!(
        input.text(&mut cx),
        "done",
        "typing over a full selection replaces everything"
    );
    assert_eq!(input.changed(), 15);
}

#[cfg(target_os = "macos")]
#[gpui::test]
fn the_macos_keys_move_to_the_ends_select_and_delete_to_the_lines_start(cx: &mut TestAppContext) {
    let (input, mut cx) = open(cx, 240., true);
    cx.simulate_input("one two");
    for (key, expected) in [
        ("ctrl-a", 0..0),
        ("ctrl-e", 7..7),
        ("home", 0..0),
        ("end", 7..7),
        ("cmd-left", 0..0),
        ("cmd-right", 7..7),
    ] {
        cx.simulate_keystrokes(key);
        assert_eq!(input.selection(&mut cx), expected, "{key}");
    }
    // The selection keeps its anchor at 7, where it started; each key moves
    // the head.
    for (key, expected) in [
        ("shift-home", 0..7),
        ("shift-end", 7..7),
        ("cmd-shift-left", 0..7),
        ("cmd-shift-right", 7..7),
        ("ctrl-shift-a", 0..7),
        ("ctrl-shift-e", 7..7),
    ] {
        cx.simulate_keystrokes(key);
        assert_eq!(input.selection(&mut cx), expected, "{key}");
    }
    assert_eq!(input.changed(), 7, "moves and selections are not changes");
    cx.simulate_keystrokes("right");
    cx.simulate_keystrokes("cmd-backspace");
    assert_eq!(
        input.text(&mut cx),
        "",
        "cmd-backspace deletes to the line's start"
    );
    assert_eq!(input.changed(), 8);
}

#[gpui::test]
fn copy_cut_and_paste_use_the_clipboard_and_pasted_line_breaks_become_spaces(
    cx: &mut TestAppContext,
) {
    let (input, mut cx) = open(cx, 240., true);
    cx.simulate_input("hello");
    cx.simulate_keystrokes("shift-left shift-left");
    assert_eq!(input.selection(&mut cx), 3..5);
    cx.simulate_keystrokes(keys::COPY);
    assert_eq!(input.changed(), 5, "copying changes nothing");
    assert_eq!(input.selection(&mut cx), 3..5);
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("lo"),
        "copy puts the selection on the clipboard"
    );
    cx.simulate_keystrokes("right");
    cx.simulate_keystrokes(keys::PASTE);
    assert_eq!(input.text(&mut cx), "hellolo");
    assert_eq!(input.changed(), 6);
    cx.simulate_keystrokes(keys::SELECT_ALL);
    cx.simulate_keystrokes(keys::CUT);
    assert_eq!(input.text(&mut cx), "", "cut removes the selection");
    assert_eq!(input.changed(), 7);
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("hellolo"),
        "cut puts the selection on the clipboard"
    );
    cx.write_to_clipboard(ClipboardItem::new_string("1\n2\n3".into()));
    cx.simulate_keystrokes(keys::PASTE);
    assert_eq!(
        input.text(&mut cx),
        "1 2 3",
        "pasted line breaks become spaces"
    );
    assert_eq!(input.changed(), 8);
}

#[gpui::test]
fn undo_restores_a_typing_run_a_paste_and_a_cut_one_step_each_and_redo_redoes(
    cx: &mut TestAppContext,
) {
    let (input, mut cx) = open(cx, 240., true);
    cx.simulate_input("one");
    cx.simulate_keystrokes("left right"); // ends the typing run
    cx.simulate_input("two");
    cx.write_to_clipboard(ClipboardItem::new_string(" THREE".into()));
    cx.simulate_keystrokes(keys::PASTE);
    cx.simulate_keystrokes(keys::SELECT_WORD_LEFT);
    cx.simulate_keystrokes(keys::CUT);
    assert_eq!(input.text(&mut cx), "onetwo ");
    cx.simulate_keystrokes(keys::UNDO);
    assert_eq!(input.text(&mut cx), "onetwo THREE", "a cut undoes alone");
    cx.simulate_keystrokes(keys::UNDO);
    assert_eq!(input.text(&mut cx), "onetwo", "a paste undoes alone");
    cx.simulate_keystrokes(keys::UNDO);
    assert_eq!(
        input.text(&mut cx),
        "one",
        "one word's typing undoes in one step"
    );
    cx.simulate_keystrokes(keys::UNDO);
    assert_eq!(input.text(&mut cx), "");
    cx.simulate_keystrokes(keys::REDO);
    assert_eq!(input.text(&mut cx), "one");
    cx.simulate_keystrokes(keys::REDO);
    assert_eq!(input.text(&mut cx), "onetwo");
    cx.simulate_keystrokes(keys::REDO);
    assert_eq!(input.text(&mut cx), "onetwo THREE");
    cx.simulate_keystrokes(keys::REDO);
    assert_eq!(input.text(&mut cx), "onetwo ");
    cx.simulate_input("x");
    cx.simulate_keystrokes(keys::REDO);
    assert_eq!(
        input.text(&mut cx),
        "onetwo x",
        "a new edit clears the redo stack"
    );
    #[cfg(not(target_os = "macos"))]
    {
        // The non-macOS table also binds `ctrl-y` to redo.
        cx.simulate_keystrokes(keys::UNDO);
        assert_eq!(input.text(&mut cx), "onetwo ");
        cx.simulate_keystrokes("ctrl-y");
        assert_eq!(input.text(&mut cx), "onetwo x");
    }
    #[cfg(target_os = "macos")]
    assert_eq!(input.changed(), 17);
    #[cfg(not(target_os = "macos"))]
    assert_eq!(input.changed(), 19);
}

#[gpui::test]
fn a_space_ends_the_typing_run_so_two_words_undo_in_two_steps(cx: &mut TestAppContext) {
    let (input, mut cx) = open(cx, 240., true);
    cx.simulate_input("one two");
    assert_eq!(input.changed(), 7, "one Changed per character");
    cx.simulate_keystrokes(keys::UNDO);
    assert_eq!(
        input.text(&mut cx),
        "one ",
        "the space ends the first word's run and undoes with it"
    );
    assert_eq!(input.changed(), 8, "an undo is one change");
    cx.simulate_keystrokes(keys::UNDO);
    assert_eq!(
        input.text(&mut cx),
        "",
        "the second word's typing is its own undo step"
    );
    assert_eq!(input.changed(), 9);
}

#[gpui::test]
fn enter_confirms_once_and_keeps_the_text_and_escape_reaches_an_ancestor(cx: &mut TestAppContext) {
    let (input, mut cx) = open(cx, 240., true);
    cx.simulate_input("hi");
    cx.simulate_keystrokes("enter");
    assert_eq!(input.confirmed(), 1, "enter confirms once");
    assert_eq!(input.text(&mut cx), "hi", "confirming keeps the text");
    assert_eq!(input.changed(), 2, "confirming is not a change");
    cx.simulate_keystrokes("escape");
    assert_eq!(
        input.escaped.get(),
        1,
        "escape reaches the ancestor's handler"
    );
    assert_eq!(input.text(&mut cx), "hi");
    assert_eq!(input.confirmed(), 1);
}

#[gpui::test]
fn composing_shows_the_marked_text_then_the_final_text_with_a_change_each(cx: &mut TestAppContext) {
    let (input, mut cx) = open(cx, 240., true);
    cx.simulate_input("xin ");
    cx.update(|window, cx| {
        input.input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "vie", None, window, cx);
            assert_eq!(
                input.marked_text_range(window, cx),
                Some(4..7),
                "the composed text is marked"
            );
        });
    });
    assert_eq!(input.text(&mut cx), "xin vie", "the composed text is shown");
    assert_eq!(input.changed(), 5, "composing is a change");
    cx.update(|window, cx| {
        input.input.update(cx, |input, cx| {
            input.replace_text_in_range(None, "việt", window, cx);
            assert_eq!(input.marked_text_range(window, cx), None);
        });
    });
    assert_eq!(
        input.text(&mut cx),
        "xin việt",
        "the final text replaces the marked text"
    );
    assert_eq!(input.changed(), 6);
}

#[gpui::test]
fn text_wider_than_the_input_keeps_the_cursor_inside_the_inputs_bounds(cx: &mut TestAppContext) {
    let (input, mut cx) = open(cx, 120., true);
    cx.simulate_input(
        "the quick brown fox jumps over the lazy dog and keeps running past the edge",
    );
    draw_a_frame(&mut cx);
    let wrapper = cx.debug_bounds("INPUT").expect("the input's wrapper draws");
    let len = input.text(&mut cx).len();
    // The cursor's reported bounds: `bounds_for_range` of an empty range at the
    // end, with the wrapper standing in for the painted element's bounds.
    let cursor = input.bounds_of(&mut cx, wrapper, len..len);
    assert!(
        cursor.left() >= wrapper.left() - px(0.5)
            && cursor.right() <= wrapper.right() + px(0.5)
            && cursor.top() >= wrapper.top() - px(0.5)
            && cursor.bottom() <= wrapper.bottom() + px(0.5),
        "the cursor stays inside the input's bounds: {cursor:?} vs {wrapper:?}"
    );
}

#[gpui::test]
fn the_mouse_places_the_cursor_selects_extends_and_selects_a_word(cx: &mut TestAppContext) {
    let (input, mut cx) = open(cx, 400., true);
    cx.simulate_input("hello");
    draw_a_frame(&mut cx);
    let wrapper = cx.debug_bounds("INPUT").expect("the input's wrapper draws");

    // A click places the cursor where the input maps the point.
    let at = input.bounds_of(&mut cx, wrapper, 1..4).center();
    let index = input.index_at(&mut cx, at);
    assert!(
        (1..=4).contains(&index),
        "the point maps inside its range: {index}"
    );
    cx.simulate_click(at, Modifiers::default());
    assert_eq!(input.selection(&mut cx), index..index);

    // A drag selects between the two points.
    let start = input.bounds_of(&mut cx, wrapper, 0..1).center();
    let end = input.bounds_of(&mut cx, wrapper, 2..3).center();
    let start_index = input.index_at(&mut cx, start);
    let end_index = input.index_at(&mut cx, end);
    assert!(
        start_index < end_index,
        "the drag spans characters: {start_index}..{end_index}"
    );
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(end, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    assert_eq!(input.selection(&mut cx), start_index..end_index);

    // A shift click extends the selection to the clicked point.
    let beyond = point(wrapper.right() - px(2.), start.y);
    let beyond_index = input.index_at(&mut cx, beyond);
    assert!(
        beyond_index > end_index,
        "the point is past the text: {beyond_index}"
    );
    cx.simulate_click(beyond, Modifiers::shift());
    assert_eq!(input.selection(&mut cx), start_index..beyond_index);

    // A double click selects the word under it.
    let word = input.bounds_of(&mut cx, wrapper, 1..3).center();
    cx.simulate_event(MouseDownEvent {
        position: word,
        modifiers: Modifiers::default(),
        button: MouseButton::Left,
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position: word,
        modifiers: Modifiers::default(),
        button: MouseButton::Left,
        click_count: 2,
    });
    assert_eq!(
        input.selection(&mut cx),
        0..5,
        "a double click selects the word"
    );
    assert_eq!(input.changed(), 5, "the mouse never changes the text");
}

#[gpui::test]
fn set_text_is_one_change_moves_the_cursor_to_the_end_and_forgets_the_history(
    cx: &mut TestAppContext,
) {
    let (input, mut cx) = open(cx, 240., true);
    cx.simulate_input("junk");
    input
        .input
        .update(&mut cx, |input, cx| input.set_text("hello", cx));
    assert_eq!(input.text(&mut cx), "hello");
    assert_eq!(
        input.selection(&mut cx),
        5..5,
        "set_text puts the cursor at the end"
    );
    assert_eq!(input.changed(), 5, "set_text is one change");
    cx.simulate_keystrokes(keys::UNDO);
    assert_eq!(
        input.text(&mut cx),
        "hello",
        "set_text forgets the undo history"
    );
    cx.simulate_input("ab");
    cx.simulate_keystrokes(keys::UNDO);
    assert_eq!(
        input.text(&mut cx),
        "hello",
        "typing after set_text undoes to it"
    );
    cx.simulate_keystrokes(keys::UNDO);
    assert_eq!(input.text(&mut cx), "hello");
    assert_eq!(input.changed(), 8);
}

#[gpui::test]
fn with_the_input_not_focused_keys_do_nothing_and_a_click_focuses_it(cx: &mut TestAppContext) {
    let (input, mut cx) = open(cx, 240., false);
    cx.simulate_input("abc");
    cx.simulate_keystrokes(&format!("{} {} {}", keys::SELECT_ALL, keys::CUT, "enter"));
    assert_eq!(input.text(&mut cx), "");
    assert_eq!(input.changed(), 0);
    assert_eq!(input.confirmed(), 0);
    // A click on the input focuses it: typing works from then on.
    draw_a_frame(&mut cx);
    let at = cx.debug_bounds("INPUT").expect("the input draws").center();
    cx.simulate_click(at, Modifiers::default());
    cx.simulate_input("a");
    assert_eq!(input.text(&mut cx), "a");
    assert_eq!(input.changed(), 1);
}

#[gpui::test]
fn the_placeholder_is_not_text(cx: &mut TestAppContext) {
    let (input, mut cx) = open(cx, 240., true);
    assert_eq!(input.text(&mut cx), "");
    cx.simulate_keystrokes(&format!("{} {}", keys::SELECT_ALL, keys::COPY));
    assert_eq!(input.selection(&mut cx), 0..0);
    assert!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .is_none(),
        "there is nothing to copy from an empty input"
    );
    cx.simulate_input("a");
    assert_eq!(input.text(&mut cx), "a");
    assert_eq!(input.changed(), 1);
}
