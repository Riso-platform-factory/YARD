use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext};
use std::{cell::RefCell, rc::Rc};
use yard_shell::{Extension, Prompt};

struct Card {
    node: String,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .debug_selector(|| format!("CARD-{}", self.node))
            .size_full()
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

/// Opens an 800×600 dev-shell window over a one-card layout, with the default
/// keys bound as the startup binds them.
fn open_app(cx: &mut TestAppContext) -> (Extension, gpui::WindowHandle<yard_shell::Shell>) {
    cx.update(yard_ui::init);
    let ext = cx.update(|cx| {
        // The startup binds the default keys in `app-init`, before extensions run.
        yard_shell::bind_default_keys(cx);
        let ext = Extension::new(cx, "tour").unwrap();
        ext.register(cx, "tour.card", &[], |node, _, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
            })
            .into()
        });
        ext.place_root(cx, "root", "yard.row");
        ext.place(cx, "a", "tour.card", "root", "children");
        ext
    });
    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(800.), gpui::px(600.)));
    draw(window, cx);
    (ext, window)
}

/// Asks `prompt` in `window` through the extension. Every answer the callback
/// gets is pushed to `answers`.
fn ask(
    ext: &Extension,
    window: gpui::WindowHandle<yard_shell::Shell>,
    cx: &mut TestAppContext,
    prompt: Prompt,
    answers: Rc<RefCell<Vec<Option<usize>>>>,
) {
    gpui::AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            ext.prompt(prompt, window, cx, move |answer, _| {
                answers.borrow_mut().push(answer);
            });
        })
        .unwrap();
}

/// Chooses `label` on the pending question and runs until the waiting task
/// has delivered the answer to the callback.
fn answer(cx: &mut TestAppContext, label: &str) {
    cx.simulate_prompt_answer(label);
    cx.run_until_parked();
}

#[gpui::test]
fn choosing_the_first_the_middle_and_the_last_button_gives_its_index_once(cx: &mut TestAppContext) {
    let (ext, window) = open_app(cx);
    for (index, label) in [(0, "Save"), (1, "Don't Save"), (2, "Cancel")] {
        let answers = Rc::new(RefCell::new(Vec::new()));
        assert!(
            !cx.has_pending_prompt(),
            "no question is pending before asking"
        );
        ask(
            &ext,
            window,
            cx,
            Prompt::new(
                gpui::PromptLevel::Warning,
                "Save changes?",
                &["Save", "Don't Save", "Cancel"],
            )
            .detail("Your changes will be lost if you don't save them."),
            answers.clone(),
        );
        assert!(cx.has_pending_prompt(), "asking shows a pending prompt");
        answer(cx, label);
        assert!(
            !cx.has_pending_prompt(),
            "the answer clears the pending prompt"
        );
        assert_eq!(
            *answers.borrow(),
            vec![Some(index)],
            "choosing `{label}` gives the callback index {index}, exactly once"
        );
    }
}

#[gpui::test]
fn a_question_without_a_detail_is_answered_the_same(cx: &mut TestAppContext) {
    let (ext, window) = open_app(cx);
    let answers = Rc::new(RefCell::new(Vec::new()));
    ask(
        &ext,
        window,
        cx,
        Prompt::new(gpui::PromptLevel::Info, "Continue?", &["Yes", "No"]),
        answers.clone(),
    );
    assert!(cx.has_pending_prompt(), "asking shows a pending prompt");
    answer(cx, "No");
    assert!(
        !cx.has_pending_prompt(),
        "the answer clears the pending prompt"
    );
    assert_eq!(*answers.borrow(), vec![Some(1)]);
}

#[gpui::test]
fn two_questions_asked_one_after_the_other_are_answered_in_order(cx: &mut TestAppContext) {
    let (ext, window) = open_app(cx);
    let first = Rc::new(RefCell::new(Vec::new()));
    let second = Rc::new(RefCell::new(Vec::new()));
    ask(
        &ext,
        window,
        cx,
        Prompt::new(gpui::PromptLevel::Info, "First question", &["OK"]),
        first.clone(),
    );
    ask(
        &ext,
        window,
        cx,
        Prompt::new(gpui::PromptLevel::Info, "Second question", &["Yes", "No"]),
        second.clone(),
    );
    assert!(
        cx.has_pending_prompt(),
        "the second question waits behind the first"
    );
    answer(cx, "OK");
    assert_eq!(
        *first.borrow(),
        vec![Some(0)],
        "the first callback got its own answer"
    );
    assert!(
        second.borrow().is_empty(),
        "the second callback was not called before its answer"
    );
    assert!(
        cx.has_pending_prompt(),
        "the second question is pending once the first is answered"
    );
    answer(cx, "No");
    assert!(!cx.has_pending_prompt(), "both questions are answered");
    assert_eq!(
        *second.borrow(),
        vec![Some(1)],
        "the second callback got its own answer"
    );
}
