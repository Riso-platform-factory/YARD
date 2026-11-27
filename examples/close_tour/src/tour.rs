use gpui::{
    App, AppContext, Context, IntoElement, ParentElement, PromptLevel, Render, Styled, Task,
    WeakEntity, Window,
};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, time::Duration};
use tracing::Instrument as _;
use yard_shell::{CloseDecision, CloseIntent, CloseOutcome, Extension, NewWindow, Prompt};
use yard_ui::{Button, Label, h_flex, palette, v_flex};

pub fn init(cx: &mut App, ext: &Extension) {
    let case = std::env::var("CLOSE_TOUR").unwrap_or_default();
    if !matches!(case.as_str(), "on" | "cancel") {
        return;
    }
    let _group = tracing::info_span!("close-tour-init", case).entered();
    if case == "cancel" {
        ext.on_close_request(cx, |intent, window, _| {
            let _group = guard_group(intent, window).entered();
            tracing::info!("always cancelling");
            CloseDecision::Cancel
        });
        return;
    }

    let cards = Rc::new(RefCell::new(BTreeMap::new()));
    let cards_on_close = cards.clone();
    cx.on_window_closed(move |_, id| {
        cards_on_close.borrow_mut().remove(&id);
    })
    .detach();

    let extension = ext.clone();
    let cards_on_open = cards.clone();
    ext.register(cx, "close_tour.card", &[], move |_, _, window, cx| {
        let card = cx.new(|_| Card {
            extension: extension.clone(),
            status: Status::Ready,
        });
        cards_on_open
            .borrow_mut()
            .insert(window.window_handle().window_id(), card.downgrade());
        card.into()
    });
    ext.place_root(cx, "close-tour-card", "close_tour.card");
    ext.on_close_request(cx, move |intent, window, cx| {
        let _group = guard_group(intent, window).entered();
        let card = cards
            .borrow()
            .get(&window.window_handle().window_id())
            .and_then(WeakEntity::upgrade);
        let Some(card) = card.filter(|card| card.read(cx).status == Status::Edited) else {
            tracing::debug!("no edits");
            return CloseDecision::Allow;
        };
        let card = card.downgrade();
        tracing::debug!("asking to save changes");
        CloseDecision::prompt(
            Prompt::new(
                PromptLevel::Warning,
                "Save changes?",
                &["Save", "Don't Save", "Cancel"],
            ),
            move |answer, window, cx| match answer {
                Some(0) => CloseOutcome::Wait(save_before_close(card, window, cx)),
                Some(1) => {
                    let _group = tracing::info_span!("close-tour-discard").entered();
                    tracing::debug!("allowed without saving");
                    CloseOutcome::Allow
                }
                _ => {
                    let _group = tracing::info_span!("close-tour-cancel").entered();
                    tracing::info!("cancelled");
                    CloseOutcome::Cancel
                }
            },
        )
    });
}

fn guard_group(intent: CloseIntent, window: &Window) -> tracing::Span {
    tracing::info_span!(
        "close-tour-guard",
        intent = ?intent,
        window = window.window_handle().window_id().as_u64()
    )
}

fn save_before_close(card: WeakEntity<Card>, window: &Window, cx: &App) -> Task<bool> {
    let group = tracing::info_span!("close-tour-save", delayed = true);
    window.spawn(cx, async move |cx| {
        async move {
            tracing::debug!("saving");
            cx.background_executor().timer(Duration::from_secs(1)).await;
            cx.update(|window, cx| {
                card.update(cx, |card, cx| card.set_status(Status::Saved, window, cx))
                    .is_ok()
            })
            .unwrap_or(false)
        }
        .instrument(group)
        .await
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Ready,
    Edited,
    Saved,
}

struct Card {
    extension: Extension,
    status: Status,
}

impl Card {
    fn set_status(&mut self, status: Status, window: &mut Window, cx: &mut Context<Self>) {
        self.extension
            .set_window_edited(window, status == Status::Edited, cx);
        self.status = status;
        tracing::debug!(status = self.label());
        cx.notify();
    }

    fn label(&self) -> &'static str {
        match self.status {
            Status::Ready => "ready",
            Status::Edited => "edited",
            Status::Saved => "saved",
        }
    }
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!("rendering close tour card");
        let edit = cx.weak_entity();
        let save = edit.clone();
        let palette = palette(cx);
        v_flex()
            .size_full()
            .p_2()
            .gap_1()
            .bg(palette.background)
            .border_1()
            .border_color(palette.border)
            .child(
                h_flex()
                    .gap_1()
                    .child(Button::new("edit", "Edit").on_click(move |window, cx| {
                        let _group = tracing::info_span!("close-tour-edit").entered();
                        let _ = edit.update(cx, |card, cx| {
                            card.set_status(Status::Edited, window, cx);
                        });
                    }))
                    .child(Button::new("save", "Save").on_click(move |window, cx| {
                        let _group =
                            tracing::info_span!("close-tour-save", delayed = false).entered();
                        let _ = save.update(cx, |card, cx| {
                            card.set_status(Status::Saved, window, cx);
                        });
                    }))
                    .child(
                        Button::new("new-window", "New window").on_click(|window, cx| {
                            let _group = tracing::info_span!("close-tour-new-window").entered();
                            tracing::debug!("requesting a window");
                            window.dispatch_action(Box::new(NewWindow), cx);
                        }),
                    ),
            )
            .child(Label::new(self.label()))
    }
}
