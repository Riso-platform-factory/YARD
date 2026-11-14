use gpui::{App, PromptLevel, SharedString, Window};
use tracing::Instrument;

/// A question drawn by the OS, with its level, its message, an optional detail,
/// and its buttons.
pub struct Prompt {
    level: PromptLevel,
    message: SharedString,
    detail: Option<SharedString>,
    answers: Vec<SharedString>,
}

impl Prompt {
    /// A question with its level, its message, and its buttons, in order.
    pub fn new(level: PromptLevel, message: impl Into<SharedString>, answers: &[&str]) -> Self {
        Self {
            level,
            message: message.into(),
            detail: None,
            answers: answers
                .iter()
                .map(|&answer| SharedString::from(answer.to_owned()))
                .collect(),
        }
    }

    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

impl crate::Extension {
    /// Asks `prompt` in `window`. `on_answer` gets the index of the chosen
    /// button, or `None` if the question went away without an answer.
    pub fn prompt(
        &self,
        prompt: Prompt,
        window: &mut Window,
        cx: &mut App,
        on_answer: impl FnOnce(Option<usize>, &mut App) + 'static,
    ) {
        let level = match prompt.level {
            PromptLevel::Info => "info",
            PromptLevel::Warning => "warning",
            PromptLevel::Critical => "critical",
        };
        let buttons = prompt.answers.len() as u64;
        let span = tracing::info_span!("prompt", level, buttons);
        let receiver = span.in_scope(|| {
            let answers: Vec<&str> = prompt.answers.iter().map(SharedString::as_ref).collect();
            window.prompt(
                prompt.level,
                &prompt.message,
                prompt.detail.as_deref(),
                &answers,
                cx,
            )
        });
        let answers = prompt.answers;
        cx.spawn(async move |cx| {
            async move {
                match receiver.await {
                    Ok(ix) => {
                        let label = answers.get(ix).map_or("", SharedString::as_ref);
                        tracing::debug!(answer = ix as u64, label, "answered");
                        cx.update(|cx| on_answer(Some(ix), cx));
                    }
                    Err(_) => {
                        tracing::info!("no answer");
                        cx.update(|cx| on_answer(None, cx));
                    }
                }
            }
            .instrument(span)
            .await
        })
        .detach();
    }
}
