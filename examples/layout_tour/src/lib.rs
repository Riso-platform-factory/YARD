//! `cargo run dev`: an empty window unless `LAYOUT_TOUR` chooses a case.
//! `LAYOUT_TOUR=card cargo run dev`: a card showing its node ID and measured size.
//! `LAYOUT_TOUR=missing cargo run dev`: an unregistered root component, logged once.
//! `LAYOUT_TOUR=twice cargo run dev`: the second root is refused; the first remains.
//! `LAYOUT_TOUR=bad-id cargo run dev`: invalid component IDs are refused.
//! `LAYOUT_TOUR=nested cargo run dev`: children in body slots, with a nested card.
//! `LAYOUT_TOUR=duplicate-id cargo run dev`: a repeated child ID is refused.
//! `LAYOUT_TOUR=missing-parent cargo run dev`: a child of an absent parent is refused.
//! `LAYOUT_TOUR=missing-slot cargo run dev`: an undeclared slot hides its subtree.
//! `LAYOUT_TOUR=missing-child cargo run dev`: an unknown child hides its subtree, not siblings.
//! `LAYOUT_TOUR=duplicate-slot cargo run dev`: duplicate slot declarations are refused.

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
        "card" | "twice" | "nested" | "duplicate-id" | "missing-parent" | "missing-slot"
        | "missing-child" | "duplicate-slot" => {
            riso_shell::register(cx, "layout_tour.card", &["body"], |node, slots, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                    size: Size::default(),
                    slots,
                })
                .into()
            });
            riso_shell::place_root(cx, "root", "layout_tour.card");
            match case.as_str() {
                "twice" => riso_shell::place_root(cx, "second", "layout_tour.card"),
                "nested" => {
                    riso_shell::place(cx, "left", "layout_tour.card", "root", "body");
                    riso_shell::place(cx, "right", "layout_tour.card", "root", "body");
                    riso_shell::place(cx, "inner", "layout_tour.card", "left", "body");
                }
                "duplicate-id" => {
                    riso_shell::place(cx, "left", "layout_tour.card", "root", "body");
                    riso_shell::place(cx, "left", "layout_tour.card", "root", "body");
                }
                "missing-parent" => {
                    riso_shell::place(cx, "lost", "layout_tour.card", "nowhere", "body")
                }
                "missing-slot" => {
                    riso_shell::place(cx, "aside", "layout_tour.card", "root", "side")
                }
                "missing-child" => {
                    riso_shell::place(cx, "left", "layout_tour.missing", "root", "body");
                    riso_shell::place(cx, "inner", "layout_tour.card", "left", "body");
                    riso_shell::place(cx, "right", "layout_tour.card", "root", "body");
                }
                "duplicate-slot" => riso_shell::register(
                    cx,
                    "layout_tour.double",
                    &["body", "body"],
                    |_, _, _, cx| cx.new(|_| gpui::Empty).into(),
                ),
                _ => {}
            }
        }
        "missing" => riso_shell::place_root(cx, "root", "layout_tour.missing"),
        "bad-id" => {
            riso_shell::register(cx, "Layout Tour.Card", &[], |_, _, _, cx| {
                cx.new(|_| gpui::Empty).into()
            });
            riso_shell::place_root(cx, "root", "Layout Tour.Card");
        }
        _ => log::warn!(
            "unknown LAYOUT_TOUR case {case:?}; known cases: card, missing, twice, bad-id, nested, duplicate-id, missing-parent, missing-slot, missing-child, duplicate-slot"
        ),
    }
}

struct Card {
    node: String,
    size: Size<Pixels>,
    slots: riso_shell::Slots,
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
            .p_2()
            .gap_1()
            .text_color(palette.text)
            .bg(palette.background)
            .child(
                div()
                    .absolute()
                    .size_full()
                    .top_0()
                    .left_0()
                    .border_1()
                    .border_color(palette.border),
            )
            .child(self.node.clone())
            .child(size)
            .child(
                div().flex().flex_col().flex_1().min_h_0().gap_1().children(
                    self.slots
                        .views("body")
                        .into_iter()
                        .map(|view| div().flex_1().min_h_0().child(view)),
                ),
            )
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
