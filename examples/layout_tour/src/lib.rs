//! The layout host, loaded as `layout_tour` in `cargo run dev`.
//! Without `LAYOUT_TOUR`, it contributes nothing.
//! `LAYOUT_TOUR=host cargo run dev`: a root card with an empty body slot.
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
//! `LAYOUT_TOUR=row cargo run dev`: three cards in a row.
//! `LAYOUT_TOUR=column cargo run dev`: three cards in a column.
//! `LAYOUT_TOUR=nested-split cargo run dev`: a column inside a row.
//! `LAYOUT_TOUR=one cargo run dev`: one card fills a row.
//! `LAYOUT_TOUR=empty-split cargo run dev`: an empty row.
//! `LAYOUT_TOUR=tabs cargo run dev`: three cards in tabs.
//! `LAYOUT_TOUR=tabs-in-split cargo run dev`: tabs inside a row.
//! `LAYOUT_TOUR=empty-tabs cargo run dev`: an empty tab bar.
//! `LAYOUT_TOUR=reserved cargo run dev`: registration in the shell's namespace is refused.

#![cfg(feature = "dev")]

#[cfg(test)]
mod tests;

use gpui::{
    App, AppContext, Bounds, Context, IntoElement, ParentElement, Pixels, Render, Size, Styled,
    WeakEntity, Window, canvas, div,
};

pub fn init(cx: &mut App, ext: &riso_shell::dev::Extension) {
    let case = std::env::var("LAYOUT_TOUR").unwrap_or_default();
    if case.is_empty() {
        return;
    }
    let _group = tracing::info_span!("layout-tour", case).entered();
    match case.as_str() {
        "row" | "column" | "nested-split" | "one" | "empty-split" | "reserved" | "tabs"
        | "tabs-in-split" | "empty-tabs" => {
            ext.register(cx, "layout_tour.card", &["body"], |node, slots, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                    size: Size::default(),
                    slots,
                })
                .into()
            });
            if case == "reserved" {
                ext.register(cx, "riso.badge", &[], |_, _, _, cx| {
                    cx.new(|_| gpui::Empty).into()
                });
                return;
            }
            ext.place_root(
                cx,
                "root",
                if case == "column" {
                    "riso.column"
                } else if case == "tabs" || case == "empty-tabs" {
                    "riso.tabs"
                } else {
                    "riso.row"
                },
            );
            if case != "empty-split" && case != "empty-tabs" {
                ext.place(cx, "a", "layout_tour.card", "root", "children");
            }
            match case.as_str() {
                "row" | "column" | "tabs" => {
                    ext.place(cx, "b", "layout_tour.card", "root", "children");
                    ext.place(cx, "c", "layout_tour.card", "root", "children");
                }
                "nested-split" | "tabs-in-split" => {
                    ext.place(
                        cx,
                        "right",
                        if case == "tabs-in-split" {
                            "riso.tabs"
                        } else {
                            "riso.column"
                        },
                        "root",
                        "children",
                    );
                    ext.place(cx, "b", "layout_tour.card", "right", "children");
                    ext.place(cx, "c", "layout_tour.card", "right", "children");
                }
                _ => {}
            }
        }
        "host" | "card" | "twice" | "nested" | "duplicate-id" | "missing-parent"
        | "missing-slot" | "missing-child" | "duplicate-slot" => {
            ext.register(cx, "layout_tour.card", &["body"], |node, slots, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                    size: Size::default(),
                    slots,
                })
                .into()
            });
            ext.place_root(cx, "root", "layout_tour.card");
            match case.as_str() {
                "twice" => ext.place_root(cx, "second", "layout_tour.card"),
                "nested" => {
                    ext.place(cx, "left", "layout_tour.card", "root", "body");
                    ext.place(cx, "right", "layout_tour.card", "root", "body");
                    ext.place(cx, "inner", "layout_tour.card", "left", "body");
                }
                "duplicate-id" => {
                    ext.place(cx, "left", "layout_tour.card", "root", "body");
                    ext.place(cx, "left", "layout_tour.card", "root", "body");
                }
                "missing-parent" => ext.place(cx, "lost", "layout_tour.card", "nowhere", "body"),
                "missing-slot" => ext.place(cx, "aside", "layout_tour.card", "root", "side"),
                "missing-child" => {
                    ext.place(cx, "left", "layout_tour.missing", "root", "body");
                    ext.place(cx, "inner", "layout_tour.card", "left", "body");
                    ext.place(cx, "right", "layout_tour.card", "root", "body");
                }
                "duplicate-slot" => ext.register(
                    cx,
                    "layout_tour.double",
                    &["body", "body"],
                    |_, _, _, cx| cx.new(|_| gpui::Empty).into(),
                ),
                _ => {}
            }
        }
        "missing" => ext.place_root(cx, "root", "layout_tour.missing"),
        "bad-id" => {
            ext.register(cx, "Layout Tour.Card", &[], |_, _, _, cx| {
                cx.new(|_| gpui::Empty).into()
            });
            ext.place_root(cx, "root", "Layout Tour.Card");
        }
        _ => log::warn!(
            "unknown LAYOUT_TOUR case {case:?}; known cases: host, card, missing, twice, \
             bad-id, nested, duplicate-id, missing-parent, missing-slot, \
             missing-child, duplicate-slot, row, column, nested-split, one, empty-split, reserved, \
             tabs, tabs-in-split, empty-tabs"
        ),
    }
}

struct Card {
    node: String,
    size: Size<Pixels>,
    slots: riso_shell::dev::Slots,
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
