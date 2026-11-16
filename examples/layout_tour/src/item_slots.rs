use super::*;
use gpui::{Entity, SharedString};
use std::{cell::RefCell, collections::BTreeMap};

struct Names(Vec<SharedString>);

impl shell::Items for Names {
    fn items(&self) -> Vec<SharedString> {
        self.0.clone()
    }
}

pub(super) fn init(cx: &mut App, ext: &shell::Extension) {
    register_card(cx, ext);
    let models = Rc::new(RefCell::new(BTreeMap::<NodeId, WeakEntity<Names>>::new()));
    let provided = models.clone();
    ext.provide::<Names>(cx, "layout_tour.names", move |node, cx| {
        let model = cx.new(|_| Names(vec!["ada".into(), "bob".into(), "cy".into()]));
        provided
            .borrow_mut()
            .insert(node.clone(), model.downgrade());
        model
    });
    ext.register(
        cx,
        "layout_tour.names",
        &["children"],
        move |node, slots, _, cx| {
            let model = models.borrow()[node]
                .upgrade()
                .expect("model created before its view");
            cx.new(|_| NamesView { model, slots }).into()
        },
    );
    ext.place_root(cx, "root", "layout_tour.names");
    ext.place(cx, "names_row", "riso.row", "root", "children");
    ext.item_slot::<Names>(cx, "names_row", "children");
    ext.place(cx, "name", "layout_tour.card", "names_row", "children");
}

struct NamesView {
    model: Entity<Names>,
    slots: shell::Slots,
}

impl Render for NamesView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!("rendering names");
        let mut controls = riso_ui::h_flex().gap_2();
        for (id, label) in [
            ("add-dee", "add dee"),
            ("remove-ada", "remove ada"),
            ("reverse", "reverse"),
        ] {
            let model = self.model.clone();
            controls = controls.child(riso_ui::Button::new(id, label).on_click(move |_, cx| {
                let _group = tracing::info_span!("names-change", action = id).entered();
                model.update(cx, |names, cx| {
                    match id {
                        "add-dee" if !names.0.iter().any(|name| name.as_str() == "dee") => {
                            names.0.push("dee".into())
                        }
                        "remove-ada" => names.0.retain(|name| name.as_str() != "ada"),
                        "reverse" => names.0.reverse(),
                        _ => {}
                    }
                    tracing::debug!(items = names.0.len(), "names changed");
                    cx.notify();
                });
            }));
        }
        riso_ui::v_flex()
            .size_full()
            .gap_2()
            .p_2()
            .child(controls)
            .child(riso_ui::Label::new(format!(
                "user layer: {}",
                shell::user_layer_len(cx)
            )))
            .children(
                self.slots
                    .views("children")
                    .into_iter()
                    .map(|view| div().flex_1().min_h_0().child(view)),
            )
    }
}
