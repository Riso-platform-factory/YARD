use gpui::{App, AppContext, Context, IntoElement, ParentElement, Render, Styled, Window};
use std::{cell::Cell, rc::Rc};
use tracing::Instrument as _;
use yard_shell::{Extension, MAX_SECRET_BYTES, SecretError};
use yard_ui::{Button, Label, palette, v_flex};

const KEY: &str = "tour-key";
const FIRST_VALUE: &[u8] = b"first tour value";
const SECOND_VALUE: &[u8] = b"replacement tour value";

pub fn init(cx: &mut App, ext: &Extension) {
    if std::env::var("SECRETS_TOUR").as_deref() != Ok("1") {
        return;
    }

    let span = tracing::info_span!("secrets-tour-init");
    let _group = span.enter();
    let cleanup = ext.delete_secret(KEY, cx);
    let extension = ext.clone();
    let wrote_secret = Rc::new(Cell::new(false));
    let wrote_before_quit = wrote_secret.clone();
    cx.on_app_quit(move |cx| {
        let cleanup = wrote_before_quit
            .get()
            .then(|| extension.delete_secret(KEY, cx));
        async move {
            if let Some(cleanup) = cleanup {
                let _ = cleanup.await;
            }
        }
    })
    .detach();

    let card = cx.new(|_| Card {
        extension: ext.clone(),
        wrote_secret,
        status: "Deleting any existing tour entry…".into(),
        pending: true,
    });
    let view = card.downgrade();
    ext.register(cx, "secrets_tour.card", &[], move |_, _, _, _| {
        card.clone().into()
    });
    ext.place_root(cx, "secrets-tour-card", "secrets_tour.card");
    tracing::debug!("placed the secrets tour card");

    let init_group = span.clone();
    cx.spawn(async move |cx| {
        async move {
            let status = match cleanup.await {
                Ok(()) => String::new(),
                Err(error) => format!("Startup cleanup · {}", error_text(error)),
            };
            let _ = view.update(cx, |card, cx| card.finish(status, cx));
        }
        .instrument(init_group)
        .await;
    })
    .detach();
}

#[derive(Clone, Copy)]
enum Action {
    Set,
    Get,
    Overwrite,
    Delete,
    Oversized,
    InvalidKey,
}

impl Action {
    fn run(self, card: &mut Card, cx: &mut Context<Card>) {
        if card.pending {
            return;
        }
        card.pending = true;
        card.status = "Working…".into();
        cx.notify();

        let extension = &card.extension;
        if matches!(self, Self::Get) {
            let task = extension.get_secret(KEY, cx);
            cx.spawn(async move |view, cx| {
                let status = match task.await {
                    Ok(Some(secret)) => match secret.expose_str() {
                        Ok(text) => format!("{} bytes · {text}", secret.expose().len()),
                        Err(_) => format!("{} bytes · invalid UTF-8", secret.expose().len()),
                    },
                    Ok(None) => "no entry".into(),
                    Err(error) => error_text(error),
                };
                let _ = view.update(cx, |card, cx| card.finish(status, cx));
            })
            .detach();
            return;
        }

        let task = match self {
            Self::Set => extension.set_secret(KEY, FIRST_VALUE, cx),
            Self::Overwrite => extension.set_secret(KEY, SECOND_VALUE, cx),
            Self::Delete => extension.delete_secret(KEY, cx),
            Self::Oversized => extension.set_secret(KEY, &[b'x'; MAX_SECRET_BYTES + 1], cx),
            Self::InvalidKey => extension.set_secret("bad key!", FIRST_VALUE, cx),
            Self::Get => unreachable!(),
        };
        let wrote_secret = card.wrote_secret.clone();
        cx.spawn(async move |view, cx| {
            let status = match task.await {
                Ok(()) => {
                    match self {
                        Self::Set | Self::Overwrite => wrote_secret.set(true),
                        Self::Delete => wrote_secret.set(false),
                        _ => {}
                    }
                    "Ok".into()
                }
                Err(error) => error_text(error),
            };
            let _ = view.update(cx, |card, cx| card.finish(status, cx));
        })
        .detach();
    }
}

fn error_text(error: SecretError) -> String {
    format!("{error:?} · {error}")
}

struct Card {
    extension: Extension,
    wrote_secret: Rc<Cell<bool>>,
    status: String,
    pending: bool,
}

impl Card {
    fn finish(&mut self, status: String, cx: &mut Context<Self>) {
        self.pending = false;
        self.status = status;
        cx.notify();
    }
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = palette(cx);
        let view = cx.weak_entity();
        let buttons = [
            ("secrets-tour-set", "Set", Action::Set),
            ("secrets-tour-get", "Get", Action::Get),
            ("secrets-tour-overwrite", "Overwrite", Action::Overwrite),
            ("secrets-tour-delete", "Delete", Action::Delete),
            (
                "secrets-tour-oversized",
                "Set 2561 bytes",
                Action::Oversized,
            ),
            (
                "secrets-tour-invalid",
                "Set invalid key",
                Action::InvalidKey,
            ),
        ]
        .map(|(id, label, action)| {
            let view = view.clone();
            Button::new(id, label)
                .disabled(self.pending)
                .on_click(move |_, cx| {
                    let _ = view.update(cx, |card, cx| action.run(card, cx));
                })
        });

        v_flex()
            .size_full()
            .p_2()
            .gap_2()
            .border_1()
            .border_color(colors.border)
            .children(buttons)
            .children((!self.status.is_empty()).then(|| Label::new(self.status.clone())))
    }
}
