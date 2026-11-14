//! The settings tour extension, loaded as `settings_tour` and `settings_guest` in `cargo run dev`.
//! Without `SETTINGS_TOUR`, it contributes nothing.
//! `SETTINGS_TOUR=card cargo run dev`: a card showing the values of `settings_tour`.
//! `SETTINGS_TOUR=guest cargo run dev`: `settings_guest` declares `settings_tour` and is refused.
//! `SETTINGS_TOUR=twice cargo run dev`: the second registration of `settings_tour` is refused.

use gpui::{App, AppContext, Context, IntoElement, ParentElement, Render, Styled, Window, div};
use riso_settings::{Settings, get};

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
        "card" | "guest" | "twice" => {
            ext.register_settings::<TourSettings>(cx);
            ext.register(cx, "settings_tour.card", &[], |_, _, _, cx| {
                cx.new(|cx| {
                    let card = cx.weak_entity();
                    let subscription = riso_settings::observe::<TourSettings>(cx, move |cx| {
                        let _ = card.update(cx, |_, cx| cx.notify());
                    });
                    Card {
                        _subscription: subscription,
                    }
                })
                .into()
            });
            if case == "twice" {
                ext.register_settings::<TourSettings>(cx);
            }
            ext.place_root(cx, "root", "settings_tour.card");
        }
        _ => log::warn!("unknown SETTINGS_TOUR case {case:?}; known cases: card, guest, twice"),
    }
}

pub fn guest_init(cx: &mut App, ext: &riso_shell::Extension) {
    let case = std::env::var("SETTINGS_TOUR").unwrap_or_default();
    if case != "guest" {
        return;
    }
    let _group = tracing::info_span!("settings-guest", case).entered();
    ext.register_settings::<TourSettings>(cx);
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
            .font(riso_ui::ui_font(cx))
            .text_size(riso_ui::ui_font_size(cx))
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
