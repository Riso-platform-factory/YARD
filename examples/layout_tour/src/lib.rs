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
//! `LAYOUT_TOUR=hide-show cargo run dev`: hide and show a counted card in a row.
//! `LAYOUT_TOUR=hide-tab cargo run dev`: hide the selected tab, then show it again.
//! `LAYOUT_TOUR=hide-root cargo run dev`: hiding the root is refused.
//! `LAYOUT_TOUR=operations cargo run dev`: add, move, and remove cards, and refuse broken rules.
//! `LAYOUT_TOUR=two-tabs cargo run dev`: two tab bars in a row, for dragging tabs between them.
//! `LAYOUT_TOUR=zoom cargo run dev`: zoom a counted card and restore shares and selection.
//! `LAYOUT_TOUR=zoom-root cargo run dev`: zooming the root is refused.
//! `LAYOUT_TOUR=user-layer cargo run dev`: rearrange, hide, and reset to the default layout.
//! `LAYOUT_TOUR=reserved cargo run dev`: registration in the shell's namespace is refused.

#[cfg(test)]
mod tests;

use gpui::{
    App, AppContext, Bounds, Context, InteractiveElement, IntoElement, ParentElement, Pixels,
    Render, Size, StatefulInteractiveElement, Styled, WeakEntity, Window, canvas, div,
};
use std::rc::Rc;

pub fn init(cx: &mut App, ext: &riso_shell::Extension) {
    let case = std::env::var("LAYOUT_TOUR").unwrap_or_default();
    if case.is_empty() {
        return;
    }
    let _group = tracing::info_span!("layout-tour", case).entered();
    match case.as_str() {
        "user-layer" => {
            register_card(cx, ext);
            let buttons = vec![
                Button::action("hide d", |ext, _, cx| ext.set_hidden(cx, "d", true)),
                Button::action("reset layout", |ext, window, cx| {
                    ext.reset_layout(window, cx)
                }),
            ];
            let handle = ext.clone();
            ext.register(cx, "layout_tour.controls", &[], move |_, _, _, cx| {
                cx.new(|_| Controls {
                    buttons: buttons.clone(),
                    ext: handle.clone(),
                })
                .into()
            });
            ext.place_root(cx, "root", "riso.row");
            ext.place(cx, "controls", "layout_tour.controls", "root", "children");
            ext.place(cx, "left", "riso.tabs", "root", "children");
            ext.place(cx, "right", "riso.tabs", "root", "children");
            for (node, parent) in [("a", "left"), ("b", "left"), ("c", "right"), ("d", "right")] {
                ext.place(cx, node, "layout_tour.card", parent, "children");
            }
        }
        "zoom-root" => {
            register_card(cx, ext);
            ext.place_root(cx, "root", "layout_tour.card");
            ext.set_zoomed(cx, "root");
        }
        "two-tabs" => {
            register_card(cx, ext);
            ext.place_root(cx, "root", "riso.row");
            ext.place(cx, "left", "riso.tabs", "root", "children");
            ext.place(cx, "right", "riso.tabs", "root", "children");
            ext.place(cx, "a", "layout_tour.card", "left", "children");
            ext.place(cx, "b", "layout_tour.card", "left", "children");
            ext.place(cx, "c", "layout_tour.card", "right", "children");
            ext.place(cx, "d", "layout_tour.card", "right", "children");
        }
        "operations" => {
            register_card(cx, ext);
            let buttons = operations_buttons();
            let handle = ext.clone();
            ext.register(cx, "layout_tour.controls", &[], move |_, _, _, cx| {
                cx.new(|_| Controls {
                    buttons: buttons.clone(),
                    ext: handle.clone(),
                })
                .into()
            });
            ext.place_root(cx, "root", "riso.row");
            ext.place(cx, "controls", "layout_tour.controls", "root", "children");
            ext.place(cx, "a", "layout_tour.card", "root", "children");
            ext.place(cx, "right", "riso.column", "root", "children");
            ext.place(cx, "b", "layout_tour.card", "right", "children");
            ext.place(cx, "c", "layout_tour.card", "right", "children");
        }
        "hide-show" | "hide-tab" | "hide-root" => {
            register_card(cx, ext);
            if case == "hide-root" {
                ext.place_root(cx, "root", "layout_tour.card");
                ext.set_hidden(cx, "root", true);
                return;
            }
            let buttons = vec![Button::visibility("b")];
            let handle = ext.clone();
            ext.register(cx, "layout_tour.controls", &[], move |_, _, _, cx| {
                cx.new(|_| Controls {
                    buttons: buttons.clone(),
                    ext: handle.clone(),
                })
                .into()
            });
            ext.place_root(cx, "root", "riso.row");
            ext.place(cx, "controls", "layout_tour.controls", "root", "children");
            let parent = if case == "hide-tab" {
                ext.place(cx, "right", "riso.tabs", "root", "children");
                "right"
            } else {
                "root"
            };
            for node in ["a", "b"] {
                ext.place(cx, node, "layout_tour.card", parent, "children");
            }
        }
        "row" | "column" | "nested-split" | "one" | "empty-split" | "reserved" | "tabs"
        | "tabs-in-split" | "empty-tabs" | "zoom" => {
            register_card(cx, ext);
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
                "nested-split" | "tabs-in-split" | "zoom" => {
                    ext.place(
                        cx,
                        "right",
                        if case == "tabs-in-split" || case == "zoom" {
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
            register_card(cx, ext);
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
             tabs, tabs-in-split, empty-tabs, hide-show, hide-tab, hide-root, operations, two-tabs, zoom, zoom-root, user-layer"
        ),
    }
}

fn register_card(cx: &mut App, ext: &riso_shell::Extension) {
    let ext_for_factory = ext.clone();
    ext.register(
        cx,
        "layout_tour.card",
        &["body"],
        move |node, slots, _, cx| {
            let card = cx.new(|_| Card {
                node: node.to_string(),
                size: Size::default(),
                count: 0,
                slots,
            });
            let ext = ext_for_factory.clone();
            cx.subscribe(&card, move |card, _: &ToggleZoom, cx| {
                let node = card.read(cx).node.clone();
                if riso_shell::zoomed(cx).as_deref() == Some(node.as_str()) {
                    ext.zoom_out(cx);
                } else {
                    ext.set_zoomed(cx, &node);
                }
            })
            .detach();
            card.into()
        },
    );
}

fn operations_buttons() -> Vec<Button> {
    vec![
        Button::action("add d", |ext, window, cx| {
            ext.add(window, cx, "d", "layout_tour.card", "root", "children");
        }),
        Button::action("move b to root", |ext, _, cx| {
            ext.move_node(cx, "b", "root", "children", usize::MAX);
        }),
        Button::action("remove a", |ext, _, cx| {
            ext.remove(cx, "a");
        }),
        Button::action("move right into c", |ext, _, cx| {
            ext.move_node(cx, "right", "c", "body", 0);
        }),
        Button::action("move c to b.side", |ext, _, cx| {
            ext.move_node(cx, "c", "b", "side", 0);
        }),
        Button::action("remove root", |ext, _, cx| {
            ext.remove(cx, "root");
        }),
    ]
}

struct Card {
    node: String,
    size: Size<Pixels>,
    count: usize,
    slots: riso_shell::Slots,
}

struct ToggleZoom;

impl gpui::EventEmitter<ToggleZoom> for Card {}

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
        let palette = riso_ui::palette(cx);
        let entity = cx.entity().downgrade();
        let zoomed = riso_shell::zoomed(cx).as_deref() == Some(self.node.as_str());
        let focus = riso_shell::focus_handle(cx, &self.node).and_then(|handle| {
            if handle.is_focused(window) {
                Some("focused")
            } else if handle.contains_focused(window, cx) {
                Some("contains focus")
            } else {
                None
            }
        });
        div()
            .debug_selector(|| format!("CARD-{}", self.node))
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
                div()
                    .id("count")
                    .debug_selector(|| format!("COUNT-{}", self.node))
                    .cursor_pointer()
                    .child(
                        div()
                            .debug_selector(|| format!("COUNT-{}-{}", self.node, self.count))
                            .child(format!("count: {}", self.count)),
                    )
                    .on_click(cx.listener(|card, _, _, cx| {
                        card.count += 1;
                        tracing::debug!(node = %card.node, count = card.count, "counted");
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("zoom")
                    .debug_selector(|| {
                        format!("ZOOM{}-{}", if zoomed { "-OUT" } else { "" }, self.node)
                    })
                    .cursor_pointer()
                    .child(if zoomed { "zoom out" } else { "zoom" })
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ToggleZoom))),
            )
            .children(focus)
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

type Label = Rc<dyn Fn(&App) -> String>;
type Action = Rc<dyn Fn(&riso_shell::Extension, &mut Window, &mut App)>;

#[derive(Clone)]
struct Button {
    label: Label,
    action: Action,
}

impl Button {
    fn action(
        label: &'static str,
        action: impl Fn(&riso_shell::Extension, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            label: Rc::new(move |_| label.into()),
            action: Rc::new(action),
        }
    }

    fn visibility(node: &'static str) -> Self {
        Self {
            label: Rc::new(move |cx| {
                let verb = if riso_shell::is_hidden(cx, node) {
                    "show"
                } else {
                    "hide"
                };
                format!("{verb} {node}")
            }),
            action: Rc::new(move |ext, _, cx| {
                let hidden = riso_shell::is_hidden(cx, node);
                ext.set_hidden(cx, node, !hidden);
            }),
        }
    }
}

struct Controls {
    buttons: Vec<Button>,
    ext: riso_shell::Extension,
}

impl Render for Controls {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!("rendering controls");
        let palette = riso_ui::palette(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .p_2()
            .gap_1()
            .text_color(palette.text)
            .bg(palette.background)
            .child(
                div()
                    .debug_selector(|| format!("USER-LAYER-{}", riso_shell::user_layer_len(cx)))
                    .child(format!("user layer: {}", riso_shell::user_layer_len(cx))),
            )
            .children(self.buttons.iter().enumerate().map(|(index, button)| {
                let label = (button.label)(cx);
                let selector = label.clone();
                let action = button.action.clone();
                let ext = self.ext.clone();
                div()
                    .id(index)
                    .debug_selector(move || format!("BUTTON-{selector}"))
                    .cursor_pointer()
                    .child(label)
                    .on_click(move |_, window, cx| action(&ext, window, cx))
            }))
    }
}
