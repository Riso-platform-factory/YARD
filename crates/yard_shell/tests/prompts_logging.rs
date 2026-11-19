use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use yard_shell::Prompt;

#[derive(Default)]
struct Captured {
    spans: BTreeMap<u64, String>,
    entered: Vec<u64>,
    lines: Vec<String>,
}

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Captured>>);

#[derive(Default)]
struct Fields(String);
impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        write!(self.0, " {}={value:?}", field.name()).unwrap();
    }
}

impl tracing::Subscriber for Trace {
    fn register_callsite(
        &self,
        _: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::sometimes()
    }
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut captured = self.0.lock().unwrap();
        let id = captured.spans.len() as u64 + 1;
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        captured
            .spans
            .insert(id, format!("{}{}", attributes.metadata().name(), fields.0));
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        let group = captured
            .entered
            .last()
            .map(|id| captured.spans[id].as_str())
            .unwrap_or("none");
        let line = format!("{group}: {}{}", event.metadata().level(), fields.0);
        captured.lines.push(line);
    }
    fn enter(&self, id: &tracing::span::Id) {
        self.0.lock().unwrap().entered.push(id.into_u64());
    }
    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(self.0.lock().unwrap().entered.pop(), Some(id.into_u64()));
    }
}

struct Diagnostics;
impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard")
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            assert_eq!(record.level(), log::Level::Error);
            tracing::error!(target: "prompts_diagnostic", "{}", record.args());
        }
    }
    fn flush(&self) {}
}

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

fn draw(window: gpui::WindowHandle<yard_shell::Shell>, app: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

/// Asks `prompt` in `window` through the extension. The callback logs a line
/// the test can find inside the question's group.
fn ask(
    ext: &yard_shell::Extension,
    window: gpui::WindowHandle<yard_shell::Shell>,
    app: &mut TestAppContext,
    prompt: Prompt,
) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            ext.prompt(prompt, window, cx, |_, _| {
                tracing::debug!("the test callback ran");
            });
        })
        .unwrap();
}

/// The innermost group enclosing `line`, or "" when no group was open.
fn group_of(line: &str) -> &str {
    line.split_once(": ").map(|(group, _)| group).unwrap_or("")
}

/// The integer value of the `name=value` field in `line`, if it is there.
fn field_value(line: &str, name: &str) -> Option<u128> {
    line.split_whitespace().find_map(|token| {
        let (field, value) = token.split_once('=')?;
        (field == name).then(|| value.parse().ok()).flatten()
    })
}

// Alone in this binary: every captured callsite runs under one scoped subscriber.
#[gpui::test]
fn asking_logs_a_prompt_group_that_stays_open_until_the_answer_comes(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        {
            // The startup binds the default keys in `app-init`, before extensions run.
            let _group = tracing::info_span!("app-init").entered();
            app.update(yard_shell::bind_default_keys);
        }
        app.update(yard_ui::init);
        let ext = app.update(|cx| {
            let ext = yard_shell::Extension::new(cx, "tour").unwrap();
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
        let window = app.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
                .unwrap()
        });
        app.simulate_window_resize(window.into(), gpui::size(gpui::px(800.), gpui::px(600.)));
        draw(window, &mut app);
        // A warning with a detail; the callback's own line follows `answered`.
        ask(
            &ext,
            window,
            &mut app,
            Prompt::new(
                gpui::PromptLevel::Warning,
                "Save changes?",
                &["Save", "Don't Save", "Cancel"],
            )
            .detail("Your changes will be lost if you don't save them."),
        );
        assert!(app.has_pending_prompt(), "asking shows a pending prompt");
        app.simulate_prompt_answer("Don't Save");
        app.run_until_parked();
        assert!(
            !app.has_pending_prompt(),
            "the answer clears the pending prompt"
        );
        // A critical question without a detail.
        ask(
            &ext,
            window,
            &mut app,
            Prompt::new(
                gpui::PromptLevel::Critical,
                "Unrecoverable failure",
                &["Quit"],
            ),
        );
        assert!(app.has_pending_prompt(), "asking shows a pending prompt");
        app.simulate_prompt_answer("Quit");
        app.run_until_parked();
        assert!(!app.has_pending_prompt());
        // Two questions asked one after the other: two groups, each with its
        // own `answered`.
        ask(
            &ext,
            window,
            &mut app,
            Prompt::new(gpui::PromptLevel::Info, "First question", &["OK"]),
        );
        ask(
            &ext,
            window,
            &mut app,
            Prompt::new(gpui::PromptLevel::Info, "Second question", &["Yes", "No"]),
        );
        assert!(
            app.has_pending_prompt(),
            "the second question waits behind the first"
        );
        app.simulate_prompt_answer("OK");
        app.run_until_parked();
        assert!(
            app.has_pending_prompt(),
            "the second question is pending once the first is answered"
        );
        app.simulate_prompt_answer("No");
        app.run_until_parked();
        assert!(!app.has_pending_prompt());
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    let lines: Vec<String> = captured
        .lines
        .iter()
        .map(|line| line.replace('"', ""))
        .collect();
    // Span creation order is group-open order.
    let ordered: Vec<String> = captured
        .spans
        .values()
        .map(|span| span.replace('"', ""))
        .collect();
    // Every ask opens its own `prompt{level, buttons}` group: `level` one of
    // `info`, `warning`, `critical`, `buttons` the number of buttons.
    for group in [
        "prompt level=warning buttons=3",
        "prompt level=critical buttons=1",
        "prompt level=info buttons=1",
        "prompt level=info buttons=2",
    ] {
        assert_eq!(
            ordered.iter().filter(|span| span.as_str() == group).count(),
            1,
            "one `{group}` group in {ordered:#?}"
        );
    }
    assert_eq!(
        ordered
            .iter()
            .filter(|span| span.starts_with("prompt"))
            .count(),
        4,
        "four `prompt` groups in total in {ordered:#?}"
    );
    // Inside its group each question logs `answered` at DEBUG with `answer`
    // and `label`, then the callback's own line.
    for (group, answer, label) in [
        ("prompt level=warning buttons=3", 1, "Don't Save"),
        ("prompt level=critical buttons=1", 0, "Quit"),
        ("prompt level=info buttons=1", 0, "OK"),
        ("prompt level=info buttons=2", 1, "No"),
    ] {
        let answered_at: Vec<usize> = (0..lines.len())
            .filter(|&position| {
                let line = &lines[position];
                group_of(line) == group
                    && line.contains(": DEBUG")
                    && line.contains("answered")
                    && field_value(line, "answer") == Some(answer)
                    && line.contains(&format!("label={label}"))
            })
            .collect();
        assert_eq!(
            answered_at.len(),
            1,
            "one `answered` line with answer={answer} label={label} in `{group}`: {lines:#?}"
        );
        assert!(
            (answered_at[0] + 1..lines.len()).any(|position| {
                let line = &lines[position];
                group_of(line) == group && line.contains("the test callback ran")
            }),
            "the callback's line is inside `{group}`, after `answered`: {lines:#?}"
        );
    }
    assert!(
        !lines.iter().any(|line| line.contains("no answer")),
        "every question was answered: {lines:#?}"
    );
    assert_eq!(
        lines.iter().filter(|line| line.contains(": ERROR")).count(),
        0,
        "nothing at ERROR in {lines:#?}"
    );
}
