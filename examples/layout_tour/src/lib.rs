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
//! `LAYOUT_TOUR=fixed-sizes cargo run dev`: a fixed side card and a flexible main card.
//! `LAYOUT_TOUR=component-sizes cargo run dev`: a card with a preferred width, one with a wide minimum, and a plain card.
//! `LAYOUT_TOUR=user-layer cargo run dev`: rearrange, hide, and reset to the default layout.
//! `LAYOUT_TOUR=actions cargo run dev`: send layout actions to the focused card.
//! `LAYOUT_TOUR=props cargo run dev`: configure cards with props, change them, and reset.
//! `LAYOUT_TOUR=titles cargo run dev`: show tab titles and icons, rename a card, and reset.
//! `LAYOUT_TOUR=tab-overflow cargo run dev`: twenty tabs that scroll along their bar, and select the last.
//! `LAYOUT_TOUR=required cargo run dev`: a required editor cannot be hidden or removed, only moved.
//! `LAYOUT_TOUR=slot-count cargo run dev`: a frame whose content slot holds one node.
//! `LAYOUT_TOUR=placement-order cargo run dev`: cards placed and moved beside named cards.
//! `LAYOUT_TOUR=contexts cargo run dev`: shared models from the nearest giving ancestor.
//! `LAYOUT_TOUR=reserved cargo run dev`: registration in the shell's namespace is refused.
//! `LAYOUT_TOUR=empty-nodes cargo run dev`: lists with no items take no room, and come back with items.
//! `LAYOUT_TOUR=item-slots cargo run dev`: cards following a list of stable item keys.
//! `LAYOUT_TOUR=named-layouts cargo run dev`: switch layouts with independent user layers.
//! `LAYOUT_TOUR=item-tabs cargo run dev`: tabs following a list of item keys, closed and added through the model.
//! `LAYOUT_TOUR=toggle cargo run dev`: status bar toggles show and hide side cards, one at a time in a column.

mod item_slots;
mod item_tabs;
mod named_layouts;
#[cfg(test)]
mod tests;

use gpui::{
    App, AppContext, Bounds, Context, InteractiveElement, IntoElement, ParentElement, Pixels,
    Render, Size, StatefulInteractiveElement, Styled, WeakEntity, Window, canvas, div,
};
use riso_layout::{NodeId, Position};
use riso_shell as shell;
use std::rc::Rc;

pub fn init(cx: &mut App, ext: &shell::Extension) {
    let case = std::env::var("LAYOUT_TOUR").unwrap_or_default();
    if case.is_empty() {
        return;
    }
    let _group = tracing::info_span!("layout-tour", case).entered();
    match case.as_str() {
        "named-layouts" => named_layouts::init(cx, ext),
        "item-slots" => item_slots::init(cx, ext),
        "item-tabs" => item_tabs::init(cx, ext),
        "toggle" => {
            register_card(cx, ext);
            let buttons = vec![Button::action("reset layout", |ext, window, cx| {
                ext.reset_layout(window, cx)
            })];
            let handle = ext.clone();
            ext.register(cx, "layout_tour.controls", &[], move |_, _, _, cx| {
                cx.new(|_| Controls {
                    buttons: buttons.clone(),
                    ext: handle.clone(),
                })
                .into()
            });
            ext.place_root(cx, "root", "riso.row");
            ext.place(cx, "side", "riso.column", "root", "children");
            ext.place(cx, "left", "layout_tour.card", "side", "children");
            ext.place(cx, "extra", "layout_tour.card", "side", "children");
            ext.place(cx, "main", "layout_tour.controls", "root", "children");
            ext.place(cx, "right", "layout_tour.card", "root", "children");
            ext.set_hidden(cx, "extra", true);
            ext.place_with_props(
                cx,
                "toggle_left",
                "riso.toggle",
                "status_bar",
                "left",
                serde_json::json!({"node": "left", "icon": "info", "alone": true}),
            );
            ext.place_with_props(
                cx,
                "toggle_extra",
                "riso.toggle",
                "status_bar",
                "left",
                serde_json::json!({"node": "extra", "icon": "plus", "alone": true}),
            );
            ext.place_with_props(
                cx,
                "toggle_right",
                "riso.toggle",
                "status_bar",
                "right",
                serde_json::json!({"node": "right", "icon": "warning"}),
            );
        }
        "empty-nodes" => {
            register_card(cx, ext);
            let lists: Lists = Rc::default();
            let created = lists.clone();
            ext.register(cx, "layout_tour.list", &[], move |node, _, _, cx| {
                let list = cx.new(|_| List {
                    node: node.to_string(),
                    items: if node.as_str() == "top" { 2 } else { 0 },
                });
                created
                    .borrow_mut()
                    .insert(node.to_string(), list.downgrade());
                list.into()
            });
            ext.register_empty::<List>(cx, "layout_tour.list");
            let buttons = ["top", "bottom"]
                .into_iter()
                .flat_map(|node| {
                    [
                        List::button(&lists, "add to", node, |items| items + 1),
                        List::button(&lists, "clear", node, |_| 0),
                    ]
                })
                .collect::<Vec<_>>();
            let handle = ext.clone();
            ext.register(cx, "layout_tour.controls", &[], move |_, _, _, cx| {
                cx.new(|_| Controls {
                    buttons: buttons.clone(),
                    ext: handle.clone(),
                })
                .into()
            });
            ext.place_root(cx, "layout", "riso.column");
            ext.place(cx, "controls", "layout_tour.controls", "layout", "children");
            ext.place(cx, "root", "riso.row", "layout", "children");
            ext.place(cx, "left", "riso.column", "root", "children");
            ext.place(cx, "top", "layout_tour.list", "left", "children");
            ext.place(cx, "bottom", "layout_tour.list", "left", "children");
            ext.place(cx, "right", "layout_tour.card", "root", "children");
        }
        "contexts" => {
            register_card(cx, ext);
            let models = Rc::new(std::cell::RefCell::new(std::collections::BTreeMap::<
                NodeId,
                WeakEntity<Count>,
            >::new()));
            let provided = models.clone();
            ext.provide::<Count>(cx, "layout_tour.counter", move |node, cx| {
                let model = cx.new(|_| Count(0));
                provided
                    .borrow_mut()
                    .insert(node.clone(), model.downgrade());
                model
            });
            ext.register(
                cx,
                "layout_tour.counter",
                &["body"],
                move |node, slots, _, cx| {
                    let model = models.borrow()[node]
                        .upgrade()
                        .expect("the node's model is created before its view");
                    cx.new(|_| Counter {
                        node: node.to_string(),
                        model,
                        slots,
                    })
                    .into()
                },
            );
            ext.require_context::<Count>(cx, "layout_tour.count_card");
            ext.register(cx, "layout_tour.count_card", &[], |node, _, _, cx| {
                cx.new(|_| CountCard {
                    node: node.to_string(),
                })
                .into()
            });
            let buttons = vec![
                Button::action("move left to root", |ext, _, cx| {
                    ext.move_node(cx, "left", "root", "children", usize::MAX);
                }),
                Button::action("move outside into counter", |ext, _, cx| {
                    ext.move_node(cx, "outside", "counter", "body", usize::MAX);
                }),
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
            ext.place_root(cx, "layout", "riso.column");
            ext.place(cx, "controls", "layout_tour.controls", "layout", "children");
            ext.place(cx, "root", "riso.row", "layout", "children");
            ext.place(cx, "counter", "layout_tour.counter", "root", "children");
            ext.place(cx, "left", "layout_tour.count_card", "counter", "body");
            ext.place(cx, "right", "layout_tour.count_card", "counter", "body");
            ext.place(cx, "outside", "layout_tour.card", "root", "children");
        }
        "placement-order" => {
            register_card(cx, ext);
            let named = |node: &str| NodeId::parse(node).unwrap();
            let buttons = vec![
                Button::action("c first", |ext, _, cx| {
                    ext.move_to(cx, "c", "root", "children", Position::First);
                }),
                Button::action("a after b", move |ext, _, cx| {
                    ext.move_to(cx, "a", "root", "children", Position::After(named("b")));
                }),
                Button::action("b before ghost", move |ext, _, cx| {
                    ext.move_to(
                        cx,
                        "b",
                        "root",
                        "children",
                        Position::Before(named("ghost")),
                    );
                }),
                Button::action("a after a", move |ext, _, cx| {
                    ext.move_to(cx, "a", "root", "children", Position::After(named("a")));
                }),
                Button::action("reset layout", |ext, window, cx| {
                    ext.reset_layout(window, cx);
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
            for node in ["a", "b", "c"] {
                ext.place(cx, node, "layout_tour.card", "root", "children");
            }
        }
        "slot-count" => {
            register_card(cx, ext);
            ext.register(
                cx,
                "layout_tour.frame",
                &["content"],
                |node, slots, _, cx| {
                    cx.new(|_| Frame {
                        node: node.to_string(),
                        slots,
                    })
                    .into()
                },
            );
            ext.slot_for_one(cx, "layout_tour.frame", "content");
            let buttons = vec![
                Button::action("add second to frame", |ext, window, cx| {
                    ext.add(window, cx, "second", "layout_tour.card", "frame", "content");
                }),
                Button::action("move loose into frame", |ext, _, cx| {
                    ext.move_node(cx, "loose", "frame", "content", usize::MAX);
                }),
                Button::action("move first to root", |ext, _, cx| {
                    ext.move_node(cx, "first", "root", "children", usize::MAX);
                }),
                Button::action("reset layout", |ext, window, cx| {
                    ext.reset_layout(window, cx);
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
            ext.place_root(cx, "layout", "riso.column");
            ext.place(cx, "controls", "layout_tour.controls", "layout", "children");
            ext.place(cx, "root", "riso.row", "layout", "children");
            ext.place(cx, "frame", "layout_tour.frame", "root", "children");
            ext.place(cx, "first", "layout_tour.card", "frame", "content");
            ext.place(cx, "loose", "layout_tour.card", "root", "children");
            ext.place(cx, "extra", "layout_tour.card", "frame", "content");
        }
        "required" => {
            register_card(cx, ext);
            register_card_as(cx, ext, "layout_tour.editor");
            ext.require(cx, "layout_tour.editor");
            let buttons = vec![
                Button::action("hide editor", |ext, _, cx| {
                    ext.set_hidden(cx, "editor", true)
                }),
                Button::action("remove main", |ext, _, cx| ext.remove(cx, "main")),
                Button::action("hide notes", |ext, _, cx| ext.set_hidden(cx, "notes", true)),
                Button::action("hide side", |ext, _, cx| ext.set_hidden(cx, "side", true)),
                Button::action("move editor to root", |ext, _, cx| {
                    ext.move_node(cx, "editor", "root", "children", usize::MAX);
                }),
                Button::action("reset layout", |ext, window, cx| {
                    ext.reset_layout(window, cx);
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
            ext.place(cx, "side", "layout_tour.card", "root", "children");
            ext.place(cx, "main", "riso.column", "root", "children");
            ext.place(cx, "editor", "layout_tour.editor", "main", "children");
            ext.place(cx, "notes", "layout_tour.card", "main", "children");
        }
        "titles" => {
            register_card(cx, ext);
            ext.require(cx, "layout_tour.card");
            ext.register_with_props::<TintedProps, TintedCard>(
                cx,
                "layout_tour.tinted",
                &[],
                |props, node, _, _, _| TintedCard {
                    node: node.to_string(),
                    props,
                },
            );
            ext.register_title::<TintedCard>(cx, "layout_tour.tinted");
            let buttons = vec![
                Button::action("rename beta", |ext, _, cx| {
                    ext.set_props(cx, "beta", serde_json::json!({"title": "Bravo"}));
                }),
                Button::action("reset layout", |ext, window, cx| {
                    ext.reset_layout(window, cx);
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
            ext.place(cx, "cards", "riso.tabs", "root", "children");
            ext.place_with_props(
                cx,
                "alpha",
                "layout_tour.tinted",
                "cards",
                "children",
                serde_json::json!({"title": "Alpha", "icon": "info"}),
            );
            ext.place_with_props(
                cx,
                "beta",
                "layout_tour.tinted",
                "cards",
                "children",
                serde_json::json!({"title": "Beta"}),
            );
            ext.place(cx, "gamma", "layout_tour.card", "cards", "children");
        }
        "props" => {
            ext.register_with_props::<TintedProps, TintedCard>(
                cx,
                "layout_tour.tinted",
                &[],
                |props, node, _, _, _| TintedCard {
                    node: node.to_string(),
                    props,
                },
            );
            let buttons = vec![
                Button::action("rename beta", |ext, _, cx| {
                    ext.set_props(cx, "beta", serde_json::json!({"title": "Beta"}));
                }),
                Button::action("tint beta warning", |ext, _, cx| {
                    ext.set_props(cx, "beta", serde_json::json!({"tint": "warning"}));
                }),
                Button::action("clear alpha's tint", |ext, _, cx| {
                    ext.set_props(cx, "alpha", serde_json::json!({"tint": null}));
                }),
                Button::action("props of ghost", |ext, _, cx| {
                    ext.set_props(cx, "ghost", serde_json::json!({"title": "Ghost"}));
                }),
                Button::action("reset layout", |ext, window, cx| {
                    ext.reset_layout(window, cx);
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
            ext.place(cx, "cards", "riso.row", "root", "children");
            ext.place_with_props(
                cx,
                "alpha",
                "layout_tour.tinted",
                "cards",
                "children",
                serde_json::json!({"title": "Alpha", "tint": "accent"}),
            );
            ext.place(cx, "beta", "layout_tour.tinted", "cards", "children");
            ext.place_with_props(
                cx,
                "broken",
                "layout_tour.tinted",
                "cards",
                "children",
                serde_json::json!({"title": "Broken", "tint": 42}),
            );
        }
        "tab-overflow" => {
            register_card(cx, ext);
            let buttons = vec![Button::action("select t20", |_, window, cx| {
                // Steps to t20 with the shell's tab actions from the focused
                // selected card, the shorter way round.
                let Some(selected) = shell::selected_tab(cx, "tabs") else {
                    return;
                };
                let Some(index) = (1..=20)
                    .map(|index| format!("t{index:02}"))
                    .position(|node| node == selected.to_string())
                else {
                    return;
                };
                if let Some(handle) = shell::focus_handle(cx, &selected.to_string()) {
                    handle.focus(window, cx);
                }
                let (steps, action): (usize, fn() -> Box<dyn gpui::Action>) = if index == 0 {
                    (1, || Box::new(shell::SelectPreviousTab))
                } else {
                    (19 - index, || Box::new(shell::SelectNextTab))
                };
                for _ in 0..steps {
                    window.dispatch_action(action(), cx);
                }
            })];
            let handle = ext.clone();
            ext.register(cx, "layout_tour.controls", &[], move |_, _, _, cx| {
                cx.new(|_| Controls {
                    buttons: buttons.clone(),
                    ext: handle.clone(),
                })
                .into()
            });
            ext.place_root(cx, "root", "riso.column");
            ext.place(cx, "controls", "layout_tour.controls", "root", "children");
            ext.place(cx, "tabs", "riso.tabs", "root", "children");
            for index in 1..=20 {
                let node = format!("t{index:02}");
                ext.place(cx, &node, "layout_tour.card", "tabs", "children");
            }
        }
        "actions" => {
            register_card(cx, ext);
            let mut buttons = vec![
                Button::action("zoom", |_, window, cx| {
                    window.dispatch_action(Box::new(shell::ToggleZoom), cx);
                }),
                Button::action("hide", |_, window, cx| {
                    window.dispatch_action(Box::new(shell::Hide), cx);
                }),
                Button::action("next tab", |_, window, cx| {
                    window.dispatch_action(Box::new(shell::SelectNextTab), cx);
                }),
                Button::action("previous tab", |_, window, cx| {
                    window.dispatch_action(Box::new(shell::SelectPreviousTab), cx);
                }),
            ];
            for button in &mut buttons {
                button.preserve_focus = true;
            }
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
            ext.place(cx, "right", "riso.tabs", "root", "children");
            for node in ["b", "c", "d"] {
                ext.place(cx, node, "layout_tour.card", "right", "children");
            }
        }
        "fixed-sizes" => {
            register_card(cx, ext);
            let buttons = vec![Button::action("reset layout", |ext, window, cx| {
                ext.reset_layout(window, cx)
            })];
            let handle = ext.clone();
            ext.register(cx, "layout_tour.controls", &[], move |_, _, _, cx| {
                cx.new(|_| Controls {
                    buttons: buttons.clone(),
                    ext: handle.clone(),
                })
                .into()
            });
            ext.place_root(cx, "root", "riso.column");
            ext.place(cx, "controls", "layout_tour.controls", "root", "children");
            ext.place_with_props(
                cx,
                "row",
                "riso.row",
                "root",
                "children",
                serde_json::json!({"sizes": {"side": 240}}),
            );
            ext.place(cx, "side", "layout_tour.card", "row", "children");
            ext.place(cx, "main", "layout_tour.card", "row", "children");
        }
        "component-sizes" => {
            register_card(cx, ext);
            register_card_as(cx, ext, "layout_tour.wide");
            register_card_as(cx, ext, "layout_tour.panel");
            ext.size_hints(
                cx,
                "layout_tour.wide",
                shell::SizeHints {
                    min: gpui::size(gpui::rems(20.), gpui::rems(6.25)),
                    preferred: gpui::size(None, None),
                },
            );
            ext.size_hints(
                cx,
                "layout_tour.panel",
                shell::SizeHints {
                    min: gpui::size(gpui::rems(5.), gpui::rems(6.25)),
                    preferred: gpui::size(Some(gpui::rems(15.)), None),
                },
            );
            let buttons = vec![Button::action("reset layout", |ext, window, cx| {
                ext.reset_layout(window, cx)
            })];
            let handle = ext.clone();
            ext.register(cx, "layout_tour.controls", &[], move |_, _, _, cx| {
                cx.new(|_| Controls {
                    buttons: buttons.clone(),
                    ext: handle.clone(),
                })
                .into()
            });
            ext.place_root(cx, "root", "riso.column");
            ext.place(cx, "controls", "layout_tour.controls", "root", "children");
            ext.place(cx, "row", "riso.row", "root", "children");
            ext.place(cx, "panel", "layout_tour.panel", "row", "children");
            ext.place(cx, "wide", "layout_tour.wide", "row", "children");
            ext.place(cx, "plain", "layout_tour.card", "row", "children");
        }
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
            "unknown LAYOUT_TOUR case {case:?}; known cases: {}{}{}titles, host, card, missing, twice, \
             bad-id, nested, duplicate-id, missing-parent, missing-slot, \
             missing-child, duplicate-slot, row, column, nested-split, one, empty-split, reserved, \
             tabs, tabs-in-split, empty-tabs, hide-show, hide-tab, hide-root, operations, two-tabs, zoom, zoom-root, user-layer, fixed-sizes, component-sizes, actions, props, required, slot-count, placement-order, contexts",
            "item-slots, item-tabs, empty-nodes, ",
            "tab-overflow, ",
            "named-layouts, "
        ),
    }
}

fn register_card(cx: &mut App, ext: &shell::Extension) {
    register_card_as(cx, ext, "layout_tour.card");
}

fn register_card_as(cx: &mut App, ext: &shell::Extension, component: &str) {
    let ext_for_factory = ext.clone();
    ext.register(cx, component, &["body"], move |node, slots, _, cx| {
        let card = cx.new(|_| Card {
            node: node.to_string(),
            size: Size::default(),
            count: 0,
            slots,
        });
        let ext = ext_for_factory.clone();
        cx.subscribe(&card, move |card, _: &ToggleZoom, cx| {
            let node = card.read(cx).node.clone();
            if shell::zoomed(cx).as_deref() == Some(node.as_str()) {
                ext.zoom_out(cx);
            } else {
                ext.set_zoomed(cx, &node);
            }
        })
        .detach();
        card.into()
    });
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum Tint {
    #[default]
    Default,
    Accent,
    Success,
    Warning,
    Error,
}

impl Tint {
    fn text_color(self) -> riso_ui::TextColor {
        match self {
            Self::Default => riso_ui::TextColor::Default,
            Self::Accent => riso_ui::TextColor::Accent,
            Self::Success => riso_ui::TextColor::Success,
            Self::Warning => riso_ui::TextColor::Warning,
            Self::Error => riso_ui::TextColor::Error,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum CardIcon {
    #[default]
    None,
    Info,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct TintedProps {
    title: String,
    tint: Tint,
    icon: CardIcon,
}

impl Default for TintedProps {
    fn default() -> Self {
        Self {
            title: "Untitled".into(),
            tint: Tint::Default,
            icon: CardIcon::None,
        }
    }
}

struct TintedCard {
    node: String,
    props: TintedProps,
}

impl shell::PropsView<TintedProps> for TintedCard {
    fn set_props(&mut self, props: TintedProps, _: &mut Context<Self>) {
        self.props = props;
    }
}

impl shell::Titled for TintedCard {
    fn title(&self, _: &App) -> gpui::SharedString {
        self.props.title.clone().into()
    }
    fn icon(&self, _: &App) -> Option<riso_ui::IconName> {
        match self.props.icon {
            CardIcon::None => None,
            CardIcon::Info => Some(riso_ui::IconName::Info),
            CardIcon::Warning => Some(riso_ui::IconName::Warning),
        }
    }
}

impl Render for TintedCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = riso_ui::palette(cx);
        div()
            .debug_selector(|| format!("CARD-{}", self.node))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .p_2()
            .gap_1()
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
            .child(
                riso_ui::Label::new(self.props.title.clone())
                    .size(riso_ui::LabelSize::Large)
                    .color(self.props.tint.text_color()),
            )
            .child(riso_ui::Label::new(self.node.clone()).color(riso_ui::TextColor::Muted))
    }
}

struct Frame {
    node: String,
    slots: shell::Slots,
}

impl Render for Frame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        riso_ui::v_flex()
            .size_full()
            .min_h_0()
            .child(
                riso_ui::Label::new(self.node.clone())
                    .size(riso_ui::LabelSize::Small)
                    .color(riso_ui::TextColor::Muted),
            )
            .children(
                self.slots
                    .views("content")
                    .into_iter()
                    .map(|view| div().flex_1().min_h_0().w_full().child(view)),
            )
    }
}

struct Card {
    node: String,
    size: Size<Pixels>,
    count: usize,
    slots: shell::Slots,
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
        let zoomed = shell::zoomed(cx).as_deref() == Some(self.node.as_str());
        let focus = shell::focus_handle(cx, &self.node).and_then(|handle| {
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
            .children(item_label(cx, &self.node))
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
type Action = Rc<dyn Fn(&shell::Extension, &mut Window, &mut App)>;

#[derive(Clone)]
struct Button {
    label: Label,
    action: Action,
    preserve_focus: bool,
}

impl Button {
    fn action(
        label: &'static str,
        action: impl Fn(&shell::Extension, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            label: Rc::new(move |_| label.into()),
            action: Rc::new(action),
            preserve_focus: false,
        }
    }

    fn visibility(node: &'static str) -> Self {
        Self {
            label: Rc::new(move |cx| {
                let verb = if shell::is_hidden(cx, node) {
                    "show"
                } else {
                    "hide"
                };
                format!("{verb} {node}")
            }),
            action: Rc::new(move |ext, _, cx| {
                let hidden = shell::is_hidden(cx, node);
                ext.set_hidden(cx, node, !hidden);
            }),
            preserve_focus: false,
        }
    }
}

struct Controls {
    buttons: Vec<Button>,
    ext: shell::Extension,
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
                    .debug_selector(|| format!("USER-LAYER-{}", shell::user_layer_len(cx)))
                    .child(format!("user layer: {}", shell::user_layer_len(cx))),
            )
            .children(self.buttons.iter().enumerate().map(|(index, button)| {
                let label = (button.label)(cx);
                let selector = label.clone();
                let action = button.action.clone();
                let ext = self.ext.clone();
                let preserve_focus = button.preserve_focus;
                div()
                    .id(index)
                    .debug_selector(move || format!("BUTTON-{selector}"))
                    .cursor_pointer()
                    .child(label)
                    .on_mouse_down(gpui::MouseButton::Left, move |_, window, _| {
                        if preserve_focus {
                            window.prevent_default();
                        }
                    })
                    .on_click(move |_, window, cx| action(&ext, window, cx))
            }))
    }
}

struct Count(usize);

struct Counter {
    node: String,
    model: gpui::Entity<Count>,
    slots: shell::Slots,
}

impl Render for Counter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!(node = %self.node, "rendering counter");
        let palette = riso_ui::palette(cx);
        let model = self.model.clone();
        riso_ui::v_flex()
            .size_full()
            .p_2()
            .gap_1()
            .border_1()
            .border_color(palette.border)
            .bg(palette.background)
            .child(
                riso_ui::h_flex()
                    .gap_2()
                    .child(riso_ui::Label::new(self.node.clone()))
                    .child(div().debug_selector(|| "CONTEXT-PLUS".into()).child(
                        riso_ui::Button::new("context-plus", "+1").on_click(move |window, cx| {
                            let _group = tracing::info_span!("count-increment").entered();
                            model.update(cx, |count, cx| {
                                count.0 += 1;
                                tracing::debug!(count = count.0, "counted");
                                cx.notify();
                            });
                            window.refresh();
                        }),
                    )),
            )
            .child(riso_ui::Label::new(format!(
                "count: {}",
                self.model.read(cx).0
            )))
            .children(
                self.slots
                    .views("body")
                    .into_iter()
                    .map(|view| div().flex_1().min_h_0().w_full().child(view)),
            )
    }
}

struct CountCard {
    node: String,
}

impl Render for CountCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!(node = %self.node, "rendering count card");
        let count = shell::context::<Count>(cx, &self.node).map(|model| model.read(cx).0);
        let palette = riso_ui::palette(cx);
        let node = self.node.clone();
        let label = count.map_or_else(|| "none".into(), |count| count.to_string());
        riso_ui::v_flex()
            .size_full()
            .p_2()
            .gap_1()
            .border_1()
            .border_color(palette.border)
            .bg(palette.background)
            .debug_selector(move || format!("CONTEXT-{node}-{label}"))
            .child(riso_ui::Label::new(self.node.clone()))
            .child(riso_ui::Label::new(
                count.map_or_else(|| "none".into(), |count| count.to_string()),
            ))
    }
}

/// The list view of each node, so that the controls can change its items.
type Lists = Rc<std::cell::RefCell<std::collections::BTreeMap<String, WeakEntity<List>>>>;

/// A list that shows its number of items, and is empty when it has none.
struct List {
    node: String,
    items: usize,
}

impl List {
    fn button(
        lists: &Lists,
        verb: &'static str,
        node: &'static str,
        change: fn(usize) -> usize,
    ) -> Button {
        let lists = lists.clone();
        let label = format!("{verb} {node}");
        Button {
            label: Rc::new(move |_| label.clone()),
            action: Rc::new(move |_, _, cx| {
                let Some(list) = lists.borrow().get(node).and_then(WeakEntity::upgrade) else {
                    return;
                };
                list.update(cx, |list, cx| {
                    list.items = change(list.items);
                    tracing::debug!(node = %list.node, items = list.items, "items changed");
                    cx.notify();
                });
            }),
            preserve_focus: false,
        }
    }
}

impl shell::Emptiable for List {
    fn is_empty(&self, _: &App) -> bool {
        self.items == 0
    }
}

impl Render for List {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!(node = %self.node, items = self.items, "rendering list");
        let palette = riso_ui::palette(cx);
        riso_ui::v_flex()
            .debug_selector(|| format!("LIST-{}", self.node))
            .size_full()
            .p_2()
            .gap_1()
            .border_1()
            .border_color(palette.border)
            .bg(palette.background)
            .child(riso_ui::Label::new(self.node.clone()))
            .child(
                riso_ui::Label::new(format!("items: {}", self.items))
                    .color(riso_ui::TextColor::Muted),
            )
            .children((1..=self.items).map(|item| riso_ui::Label::new(format!("item {item}"))))
    }
}

fn item_label(cx: &App, node: &str) -> Option<riso_ui::Label> {
    shell::item(cx, node).map(riso_ui::Label::new)
}
