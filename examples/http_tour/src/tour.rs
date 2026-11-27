use gpui::{App, AppContext, Context, IntoElement, ParentElement, Render, Styled, Window};
use std::time::Instant;
use tracing::Instrument as _;
use yard_shell::{
    Extension,
    yard_http::{
        http_client::{AsyncBody, HttpClient as _},
        read_limited,
    },
};
use yard_ui::{Button, Label, palette, v_flex};

pub fn init(cx: &mut App, ext: &Extension) {
    let Ok(url) = std::env::var("HTTP_TOUR") else {
        return;
    };
    if url.is_empty() {
        return;
    }
    let _group = tracing::info_span!("http-tour-init").entered();
    let extension = ext.clone();
    ext.register(cx, "http_tour.card", &[], move |_, _, _, cx| {
        cx.new(|_| Card {
            extension: extension.clone(),
            url: url.clone(),
            status: String::new(),
            pending: false,
        })
        .into()
    });
    ext.place_root(cx, "http-tour-card", "http_tour.card");
    tracing::debug!("placed the HTTP tour card");
}

struct Card {
    extension: Extension,
    url: String,
    status: String,
    pending: bool,
}

impl Card {
    fn fetch(&mut self, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let span = tracing::info_span!("http-tour-fetch");
        let _group = span.enter();
        let http = self.extension.http_client(cx);
        let url = self.url.clone();
        self.status = "Fetching…".into();
        self.pending = true;
        cx.notify();

        let task = cx.background_spawn(
            async move {
                let started = Instant::now();
                let result = async {
                    let response = http.get(&url, AsyncBody::empty(), true).await?;
                    let status = response.status();
                    read_limited(response, 1 << 20)
                        .await
                        .map(|body| (status, body.len()))
                }
                .await;
                let elapsed_ms = started.elapsed().as_millis();
                tracing::debug!(ok = result.is_ok(), "fetch finished");
                match result {
                    Ok((status, bytes)) => format!("{status} · {bytes} bytes · {elapsed_ms} ms"),
                    Err(error) => format!("{error} · {elapsed_ms} ms"),
                }
            }
            .instrument(span.clone()),
        );
        cx.spawn(async move |view, cx| {
            let status = task.await;
            let _ = view.update(cx, |card, cx| {
                card.pending = false;
                card.status = status;
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = palette(cx);
        let view = cx.weak_entity();
        v_flex()
            .size_full()
            .p_2()
            .gap_2()
            .border_1()
            .border_color(colors.border)
            .child(
                Button::new("http-tour-fetch", "Fetch")
                    .disabled(self.pending)
                    .on_click(move |_, cx| {
                        let _ = view.update(cx, |card, cx| card.fetch(cx));
                    }),
            )
            .children((!self.status.is_empty()).then(|| Label::new(self.status.clone())))
    }
}
