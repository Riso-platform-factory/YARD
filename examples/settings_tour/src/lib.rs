#![cfg(feature = "dev")]

use gpui::{App, AppContext, Context, IntoElement, ParentElement, Render, Styled, Window, div};
use riso_settings::dev::{Settings, get, register};

#[derive(serde::Deserialize)]
#[serde(default)]
struct TourSettings {
    greeting: String,
    size: u32,
    enabled: bool,
}

impl Default for TourSettings {
    fn default() -> Self {
        Self {
            greeting: "hello".into(),
            size: 14,
            enabled: true,
        }
    }
}

impl Settings for TourSettings {
    const KEY: &'static str = "settings_tour";
}

pub fn init(cx: &mut App, ext: &riso_shell::Extension) {
    let case = std::env::var("SETTINGS_TOUR").unwrap_or_default();
    if case.is_empty() {
        return;
    }
    let _group = tracing::info_span!("settings-tour", case).entered();
    match case.as_str() {
        "card" | "twice" => {
            register::<TourSettings>(cx);
            ext.register(cx, "settings_tour.card", &[], |_, _, _, cx| {
                cx.new(|cx| {
                    let card = cx.weak_entity();
                    let subscription = riso_settings::dev::observe::<TourSettings>(cx, move |cx| {
                        let _ = card.update(cx, |_, cx| cx.notify());
                    });
                    Card {
                        _subscription: subscription,
                    }
                })
                .into()
            });
            if case == "twice" {
                register::<TourSettings>(cx);
            }
            ext.place_root(cx, "root", "settings_tour.card");
        }
        _ => log::warn!("unknown SETTINGS_TOUR case {case:?}; known cases: card, twice"),
    }
}

struct Card {
    _subscription: gpui::Subscription,
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!("rendering card");
        let settings = get::<TourSettings>(cx);
        let palette = riso_ui::palette(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .p_2()
            .gap_1()
            .font(riso_ui::ui_font())
            .text_size(riso_ui::ui_font_size())
            .text_color(palette.text)
            .bg(palette.background)
            .border_1()
            .border_color(palette.border)
            .child("settings_tour")
            .child(format!("greeting: {}", settings.greeting))
            .child(format!("size: {}", settings.size))
            .child(format!("enabled: {}", settings.enabled))
    }
}
