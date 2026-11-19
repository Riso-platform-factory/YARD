//! The layout guest, loaded as `layout_guest` in `cargo run dev`.
//! Without `LAYOUT_GUEST`, it contributes nothing.
//! `LAYOUT_TOUR=host LAYOUT_GUEST=card cargo run dev`: a guest card in the host's body.
//! `LAYOUT_TOUR=host LAYOUT_GUEST=foreign cargo run dev`: a foreign component is refused.
//! `LAYOUT_TOUR=host LAYOUT_GUEST=namespace cargo run dev`: foreign registration is refused.
//! `LAYOUT_TOUR=row LAYOUT_GUEST=split cargo run dev`: a fourth card in the host row.
//! `LAYOUT_TOUR=tabs LAYOUT_GUEST=tab cargo run dev`: a fourth tab in the host.
//! `LAYOUT_GUEST=card cargo run dev`: the absent host's parent is refused.
//! `LAYOUT_TOUR=placement-order cargo run dev`: a guest card placed before the host's `b`.

use gpui::{App, AppContext, Context, IntoElement, ParentElement, Render, Styled, Window, div};
use yard_layout::{NodeId, Position};

pub fn init(cx: &mut App, ext: &yard_shell::Extension) {
    let mut case = std::env::var("LAYOUT_GUEST").unwrap_or_default();
    // The guest's part of the host's placement-order case needs no case of its own.
    if case.is_empty() && std::env::var("LAYOUT_TOUR").as_deref() == Ok("placement-order") {
        case = "placement-order".into();
    }
    if case.is_empty() {
        return;
    }
    let _group = tracing::info_span!("layout-guest", case).entered();
    match case.as_str() {
        "card" | "split" | "tab" => {
            ext.register(cx, "layout_guest.card", &[], |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                })
                .into()
            });
            ext.place(
                cx,
                "guest",
                "layout_guest.card",
                "root",
                if case == "split" || case == "tab" {
                    "children"
                } else {
                    "body"
                },
            );
        }
        "placement-order" => {
            ext.register(cx, "layout_guest.card", &[], |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                })
                .into()
            });
            ext.place_at(
                cx,
                "guest",
                "layout_guest.card",
                "root",
                "children",
                Position::Before(NodeId::parse("b").unwrap()),
            );
        }
        "foreign" => ext.place(cx, "guest", "layout_tour.card", "root", "body"),
        "namespace" => {
            ext.register(cx, "layout_tour.badge", &[], |_, _, _, cx| {
                cx.new(|_| gpui::Empty).into()
            });
        }
        _ => {
            log::warn!(
                "unknown LAYOUT_GUEST case {case:?}; known cases: card, foreign, namespace, split, tab"
            )
        }
    }
}

struct Card {
    node: String,
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!(node = %self.node, "rendering guest card");
        let palette = yard_ui::palette(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .border_1()
            .border_color(palette.border)
            .p_2()
            .text_color(palette.text)
            .bg(palette.background)
            .child(self.node.clone())
            .child("from layout_guest")
    }
}
