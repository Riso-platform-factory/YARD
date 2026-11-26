use gpui::{
    App, AppContext, Context, IntoElement, ParentElement, Render, Styled, SystemNotification,
    SystemNotificationAction, Window,
};
use std::{cell::RefCell, rc::Rc, time::Duration};
use tracing::Instrument;
use yard_shell::Extension;
use yard_ui::{Button, Label, palette, v_flex};

const CASES: &[&str] = &["simple", "actions", "replace", "dismiss", "away"];

#[derive(Default)]
struct State {
    count: u32,
    response: Option<String>,
    status: Option<String>,
}

type Shared = Rc<RefCell<State>>;

pub fn init(cx: &mut App, ext: &Extension) {
    let case = std::env::var("NOTIFICATION_TOUR").unwrap_or_default();
    if case.is_empty() {
        return;
    }
    let _group = tracing::info_span!("notification-tour", case).entered();
    let Some(&case) = CASES.iter().find(|known| **known == case) else {
        log::warn!(
            "unknown NOTIFICATION_TOUR case {case:?}; known cases: {}",
            CASES.join(", ")
        );
        return;
    };
    let state = Shared::default();
    let responses = state.clone();
    ext.on_notification_response(cx, move |response, cx| {
        let answer = response.action_id.as_deref().unwrap_or("body");
        responses.borrow_mut().response = Some(format!("{answer}, tag {}", response.tag));
        cx.refresh_windows();
    });
    let card_ext = ext.clone();
    ext.register(cx, "notification_tour.card", &[], move |_, _, _, cx| {
        cx.new(|_| Card {
            ext: card_ext.clone(),
            case,
            state: state.clone(),
        })
        .into()
    });
    ext.place_root(cx, "root", "notification_tour.card");
}

struct Card {
    ext: Extension,
    case: &'static str,
    state: Shared,
}

impl Card {
    fn button(
        &self,
        id: &'static str,
        label: &'static str,
        press: fn(&Extension, &Shared, &mut App),
    ) -> Button {
        let ext = self.ext.clone();
        let state = self.state.clone();
        Button::new(id, label).on_click(move |_, cx| press(&ext, &state, cx))
    }

    fn buttons(&self) -> Vec<Button> {
        match self.case {
            "simple" => vec![self.button("post", "Post", |ext, _, cx| {
                ext.show_notification(
                    notification("hello", "Hello", "From the notification tour", &[]),
                    cx,
                )
            })],
            "actions" => vec![self.button("post", "Post with buttons", |ext, _, cx| {
                ext.show_notification(
                    notification(
                        "message",
                        "New message",
                        "Reply, or mark it read",
                        &[("reply", "Reply"), ("read", "Mark read")],
                    ),
                    cx,
                )
            })],
            "replace" => vec![self.button("post", "Post count", |ext, state, cx| {
                let count = {
                    let mut state = state.borrow_mut();
                    state.count += 1;
                    state.count
                };
                ext.show_notification(
                    notification("counter", "Counter", &format!("Count {count}"), &[]),
                    cx,
                )
            })],
            "dismiss" => vec![
                self.button("post", "Post", |ext, _, cx| {
                    ext.show_notification(
                        notification(
                            "dismissable",
                            "Dismissable",
                            "Dismiss me from the tour",
                            &[],
                        ),
                        cx,
                    )
                }),
                self.button("dismiss", "Dismiss", |ext, _, cx| {
                    ext.dismiss_notification("dismissable", cx)
                }),
            ],
            _ => vec![self.button("post", "Post in five seconds", post_away)],
        }
    }
}

/// Posts after five seconds, only when no window of the application is in front.
fn post_away(ext: &Extension, state: &Shared, cx: &mut App) {
    let span = tracing::info_span!("notification-tour-away");
    let ext = ext.clone();
    let state = state.clone();
    state.borrow_mut().status = Some("waiting five seconds".into());
    cx.refresh_windows();
    cx.spawn(async move |cx| {
        async {
            cx.background_executor().timer(Duration::from_secs(5)).await;
            cx.update(|cx| {
                // On macOS `active_window` may stay set while the application is inactive.
                let in_front = cx.windows().into_iter().any(|handle| {
                    handle
                        .update(cx, |_, window, _| window.is_window_active())
                        .unwrap_or(false)
                });
                let status = if !in_front {
                    tracing::info!("posted: no window is in front");
                    ext.show_notification(
                        notification("away", "While you were away", "Posted from the tour", &[]),
                        cx,
                    );
                    "posted"
                } else {
                    tracing::info!("not posted: a window is in front");
                    "not posted: a window is in front"
                };
                state.borrow_mut().status = Some(status.into());
                cx.refresh_windows();
            });
        }
        .instrument(span)
        .await
    })
    .detach();
}

fn notification(
    tag: &'static str,
    title: &'static str,
    body: &str,
    actions: &[(&'static str, &'static str)],
) -> SystemNotification {
    SystemNotification {
        tag: tag.into(),
        title: title.into(),
        body: body.to_string().into(),
        actions: actions
            .iter()
            .map(|&(id, label)| SystemNotificationAction {
                id: id.into(),
                label: label.into(),
            })
            .collect(),
    }
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!("rendering notification card");
        let palette = palette(cx);
        let state = self.state.borrow();
        let response = state.response.as_deref().unwrap_or("none");
        v_flex()
            .size_full()
            .p_2()
            .gap_1()
            .bg(palette.background)
            .border_1()
            .border_color(palette.border)
            .children(self.buttons())
            .child(Label::new(format!("last response: {response}")))
            .children(state.status.clone().map(Label::new))
    }
}
