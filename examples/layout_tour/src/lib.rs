//! `cargo run dev`: an empty window unless `LAYOUT_TOUR` chooses a case.
//! `LAYOUT_TOUR=card cargo run dev`: a card showing its node ID and measured size.
//! `LAYOUT_TOUR=missing cargo run dev`: an unregistered root component, logged once.
//! `LAYOUT_TOUR=twice cargo run dev`: the second root is refused; the first remains.
//! `LAYOUT_TOUR=bad-id cargo run dev`: invalid component IDs are refused.

#[cfg(test)]
mod tests;

use gpui::{
    App, AppContext, Bounds, Context, IntoElement, ParentElement, Pixels, Render, Size, Styled,
    WeakEntity, Window, canvas, div,
};

pub fn init(cx: &mut App) {
    let case = std::env::var("LAYOUT_TOUR").unwrap_or_default();
    if case.is_empty() {
        return;
    }
    let _group = tracing::info_span!("layout-tour", case).entered();
    match case.as_str() {
        "card" | "twice" => {
            riso_shell::register(cx, "layout_tour.card", |node, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                    size: Size::default(),
                })
                .into()
            });
            riso_shell::place_root(cx, "root", "layout_tour.card");
            if case == "twice" {
                riso_shell::place_root(cx, "second", "layout_tour.card");
            }
        }
        "missing" => riso_shell::place_root(cx, "root", "layout_tour.missing"),
        "bad-id" => {
            riso_shell::register(cx, "Layout Tour.Card", |_, _, cx| {
                cx.new(|_| gpui::Empty).into()
            });
            riso_shell::place_root(cx, "root", "Layout Tour.Card");
        }
        _ => log::warn!(
            "unknown LAYOUT_TOUR case {case:?}; known cases: card, missing, twice, bad-id"
        ),
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = format!("{} × {}", self.size.width, self.size.height);
        tracing::trace!(node = %self.node, size = %size, "rendering card");
        let palette = riso_ui::palette(cx);
        let entity = cx.entity().downgrade();
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .text_color(palette.text)
            .bg(palette.background)
            .child(self.node.clone())
            .child(size)
            .child(
                canvas(
                    move |bounds, window, cx| Self::prepaint(entity, bounds, window, cx),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
    }
}
