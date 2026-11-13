//! The window tour extension, loaded as `window_tour` in `cargo run dev`.
//! Without `WINDOW_TOUR`, it contributes nothing.
//! `WINDOW_TOUR=title-bar cargo run dev`: a root card, `window_tour` in `left`, and `ping` in `right`.

use gpui::{
    App, AppContext, Bounds, Context, InteractiveElement, IntoElement, ParentElement, Pixels,
    Render, Size, Styled, WeakEntity, Window, canvas, div, rems,
};
use riso_ui::{Button, ButtonSize, Label, LabelSize, palette, v_flex};

pub fn init(cx: &mut App, ext: &riso_shell::Extension) {
    let case = std::env::var("WINDOW_TOUR").unwrap_or_default();
    if case.is_empty() {
        return;
    }
    let _group = tracing::info_span!("window-tour", case).entered();
    match case.as_str() {
        "title-bar" => {
            ext.register(cx, "window_tour.card", &[], |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                    size: Size::default(),
                })
                .into()
            });
            ext.register(cx, "window_tour.title", &[], |_, _, _, cx| {
                cx.new(|_| BarLabel).into()
            });
            ext.register(cx, "window_tour.ping", &[], |_, _, _, cx| {
                cx.new(|_| PingButton).into()
            });
            ext.place_root(cx, "root", "window_tour.card");
            ext.place(cx, "title", "window_tour.title", "title_bar", "left");
            ext.place(cx, "ping", "window_tour.ping", "title_bar", "right");
        }
        _ => log::warn!("unknown WINDOW_TOUR case {case:?}; known cases: title-bar"),
    }
}

struct BarLabel;

impl Render for BarLabel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .debug_selector(|| "BAR-TITLE".into())
            .child(Label::new("window_tour").size(LabelSize::Small))
    }
}

struct PingButton;

impl Render for PingButton {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().debug_selector(|| "BAR-PING".into()).child(
            Button::new("ping", "ping")
                .size(ButtonSize::Compact)
                .on_click(|_, _| tracing::info!("ping")),
        )
    }
}

struct Card {
    node: String,
    size: Size<Pixels>,
}

impl Card {
    fn prepaint(
        entity: WeakEntity<Self>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let changed = entity
            .update(cx, |card, _| {
                if card.size == bounds.size {
                    return false;
                }
                tracing::trace!(
                    node = %card.node,
                    width = %bounds.size.width,
                    height = %bounds.size.height,
                    "card size changed"
                );
                card.size = bounds.size;
                true
            })
            .unwrap_or(false);
        if changed {
            // Prepaint invalidations do not schedule a redraw in GPUI.
            window.on_next_frame(move |_, cx| {
                let _ = entity.update(cx, |_, cx| cx.notify());
            });
        }
    }
}

impl Render for Card {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = format!("{} × {}", self.size.width, self.size.height);
        tracing::trace!(node = %self.node, size = %size, "rendering card");
        let palette = palette(cx);
        let entity = cx.entity().downgrade();
        let focused = riso_shell::focus_handle(cx, &self.node)
            .is_some_and(|handle| handle.is_focused(window));
        v_flex()
            .debug_selector(|| format!("CARD-{}", self.node))
            .relative()
            .size_full()
            .p(rems(0.5))
            .gap(rems(0.25))
            .bg(palette.background)
            .border_1()
            .border_color(palette.border)
            .child(Label::new(self.node.clone()))
            .child(Label::new(size))
            .children(focused.then(|| Label::new("focused")))
            .child(
                canvas(
                    move |bounds, window, cx| Self::prepaint(entity, bounds, window, cx),
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
    }
}
