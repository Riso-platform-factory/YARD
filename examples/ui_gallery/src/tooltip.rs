use gpui::{App, AppContext, Context, IntoElement, ParentElement, Render, Styled, Window, rems};
use riso_ui::{Button, IconButton, IconName, Tooltip, h_flex, v_flex};

pub(super) fn register(cx: &mut App, ext: &riso_shell::Extension) {
    ext.register(cx, "ui_gallery.tooltip", &[], |_, _, _, cx| {
        cx.new(|_| TooltipPage).into()
    });
}

pub(super) fn place(cx: &mut App, ext: &riso_shell::Extension) {
    ext.place(cx, "tooltip", "ui_gallery.tooltip", "root", "children");
    tracing::debug!(page = "tooltip", "page placed");
}

struct TooltipPage;

impl Render for TooltipPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!(page = "tooltip", "rendering page");
        let palette = riso_ui::palette(cx);
        let text = Button::new("tooltip-text", "Text").tooltip(Tooltip::text("Save the file"));
        let keys = Button::new("tooltip-keys", "Keys")
            .tooltip(Tooltip::for_action("Quit the gallery", &riso_shell::Quit));
        let detail = Button::new("tooltip-detail", "Detail").tooltip(Tooltip::with_meta(
            "Save the file",
            None,
            "Keep a copy in the current folder",
        ));
        let icon = IconButton::new("tooltip-icon", IconName::Info)
            .tooltip(Tooltip::text("More information"));
        let disabled = Button::new("tooltip-disabled", "Disabled")
            .disabled(true)
            .tooltip(Tooltip::text("This action is unavailable"));
        let long = Button::new("tooltip-long", "Long").tooltip(Tooltip::text(
            "Save the file to disk and keep a backup copy of the previous version \
             in the same folder",
        ));
        v_flex()
            .size_full()
            .p(rems(1.))
            .gap(rems(0.75))
            .bg(palette.background)
            .child(crate::group(
                "Kinds",
                h_flex()
                    .flex_wrap()
                    .gap(rems(0.75))
                    .child(text)
                    .child(keys)
                    .child(detail),
            ))
            .child(crate::group(
                "States",
                h_flex()
                    .flex_wrap()
                    .gap(rems(0.75))
                    .child(icon)
                    .child(disabled),
            ))
            .child(crate::group("Wrapping", h_flex().child(long)))
    }
}
