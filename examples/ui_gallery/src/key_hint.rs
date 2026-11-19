use gpui::{
    Action, App, AppContext, Context, IntoElement, ParentElement, Render, Styled, Window, rems,
};
use yard_ui::{KeyHint, KeyHintStyle, Label, ListItem, ListItemSpacing, h_flex, v_flex};

pub(crate) fn register(cx: &mut App, ext: &yard_shell::Extension) {
    ext.register(cx, "ui_gallery.key_hint", &[], |_, _, _, cx| {
        cx.new(|_| KeyHintPage).into()
    });
}

pub(crate) fn place(cx: &mut App, ext: &yard_shell::Extension) {
    ext.place(cx, "key-hint", "ui_gallery.key_hint", "root", "children");
    tracing::debug!(page = "key-hint", "page placed");
}

struct KeyHintPage;

impl Render for KeyHintPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!(page = "key-hint", "rendering page");
        let actions: [&dyn Action; 8] = [
            &yard_shell::ToggleZoom,
            &yard_shell::SelectNextTab,
            &yard_shell::SelectPreviousTab,
            &yard_shell::FocusLeft,
            &yard_shell::FocusRight,
            &yard_shell::FocusUp,
            &yard_shell::FocusDown,
            &yard_shell::Quit,
        ];
        let palette = yard_ui::palette(cx);
        v_flex()
            .size_full()
            .p(rems(1.))
            .gap(rems(0.75))
            .bg(palette.background)
            .child(crate::group(
                "Default",
                v_flex()
                    .gap(rems(0.25))
                    .children(actions.iter().enumerate().map(|(index, action)| {
                        ListItem::new(("key-hint-default", index))
                            .inset(true)
                            .spacing(ListItemSpacing::Sparse)
                            .child(
                                h_flex()
                                    .w_full()
                                    .py_px()
                                    .justify_between()
                                    .gap_2()
                                    .child(Label::new(action.name()))
                                    .child(
                                        h_flex()
                                            .flex_shrink_0()
                                            .gap_2()
                                            .child(KeyHint::for_action(*action, cx)),
                                    ),
                            )
                    })),
            ))
            .child(crate::group(
                "Label",
                v_flex()
                    .gap(rems(0.25))
                    .children(actions.iter().enumerate().map(|(index, action)| {
                        ListItem::new(("key-hint-label", index))
                            .inset(true)
                            .spacing(ListItemSpacing::Sparse)
                            .child(
                                h_flex()
                                    .w_full()
                                    .py_px()
                                    .justify_between()
                                    .gap_2()
                                    .child(Label::new(action.name()))
                                    .child(h_flex().flex_shrink_0().gap_2().child(
                                        KeyHint::for_action(*action, cx).style(KeyHintStyle::Label),
                                    )),
                            )
                    }))
                    .child(
                        ListItem::new("key-hint-disabled")
                            .inset(true)
                            .spacing(ListItemSpacing::Sparse)
                            .child(
                                h_flex()
                                    .w_full()
                                    .py_px()
                                    .justify_between()
                                    .gap_2()
                                    .child(Label::new("Disabled"))
                                    .child(
                                        h_flex().flex_shrink_0().gap_2().child(
                                            KeyHint::for_action(&yard_shell::Quit, cx)
                                                .style(KeyHintStyle::Label)
                                                .disabled(true),
                                        ),
                                    ),
                            ),
                    ),
            ))
    }
}
