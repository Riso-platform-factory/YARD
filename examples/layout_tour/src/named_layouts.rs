use super::*;

pub(super) fn init(cx: &mut App, ext: &shell::Extension) {
    register_card(cx, ext);
    let handle = ext.clone();
    ext.register(cx, "layout_tour.named_controls", &[], move |_, _, _, cx| {
        cx.new(|_| NamedControls {
            ext: handle.clone(),
        })
        .into()
    });
    ext.place_root(cx, "root", "riso.row");
    ext.place(cx, "cards", "riso.row", "root", "children");
    for node in ["a", "b", "c"] {
        ext.place(cx, node, "layout_tour.card", "cards", "children");
    }
    ext.place(
        cx,
        "controls",
        "layout_tour.named_controls",
        "root",
        "children",
    );
    ext.named_layout(cx, "layout_tour.swap", |layout| {
        layout.move_node("c", "cards", "children", 0);
    });
    ext.named_layout(cx, "layout_tour.focus", |layout| {
        layout.set_hidden("a", true);
        layout.set_hidden("c", true);
        layout.add("d", "layout_tour.card", "cards", "children");
    });
}

struct NamedControls {
    ext: shell::Extension,
}

impl Render for NamedControls {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!("rendering named layout controls");
        let ext = self.ext.clone();
        let reset = self.ext.clone();
        riso_ui::v_flex()
            .size_full()
            .p_2()
            .gap_2()
            .child(riso_ui::Label::new(format!(
                "shown: {}",
                shell::shown_layout(cx, window)
            )))
            .children(
                [
                    ("default", "riso.default"),
                    ("swap", "layout_tour.swap"),
                    ("focus", "layout_tour.focus"),
                ]
                .into_iter()
                .map(|(label, name)| {
                    let ext = self.ext.clone();
                    riso_ui::Button::new(label, label)
                        .on_click(move |window, cx| ext.show_layout(window, cx, name))
                }),
            )
            .child(
                riso_ui::Button::new("hide-c", "hide c")
                    .on_click(move |_, cx| ext.set_hidden(cx, "c", true)),
            )
            .child(
                riso_ui::Button::new("reset-layout", "reset layout")
                    .on_click(move |window, cx| reset.reset_layout(window, cx)),
            )
    }
}
