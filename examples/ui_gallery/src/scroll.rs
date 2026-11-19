use gpui::{
    App, AppContext, Context, IntoElement, ParentElement, Render, ScrollHandle, ScrollStrategy,
    Styled, UniformListScrollHandle, Window, px, rems,
};
use yard_ui::{Button, ButtonStyle, Label, ListItem, ScrollArea, ScrollList, h_flex, v_flex};

const PARAGRAPH_COUNT: usize = 40;
const ROW_COUNT: usize = 1000;

pub(super) fn register(cx: &mut App, ext: &yard_shell::Extension) {
    ext.register(cx, "ui_gallery.scroll", &[], |_, _, _, cx| {
        cx.new(|_| ScrollPage::new()).into()
    });
}

struct ScrollPage {
    area_handle: ScrollHandle,
    list_handle: UniformListScrollHandle,
}

impl ScrollPage {
    fn new() -> Self {
        Self {
            area_handle: ScrollHandle::new(),
            list_handle: UniformListScrollHandle::new(),
        }
    }
}

impl Render for ScrollPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!(page = "scroll", "rendering page");
        let palette = yard_ui::palette(cx);
        let page = cx.weak_entity();

        let area = ScrollArea::new("scroll-area", &self.area_handle)
            .w_full()
            .h(px(300.))
            .border_1()
            .border_color(palette.border)
            .child(
                v_flex()
                    .w_full()
                    .p(rems(0.5))
                    .gap(rems(0.5))
                    .children((0..PARAGRAPH_COUNT).map(|index| {
                        Label::new(format!(
                            "Paragraph {}: A scrollable column holds arbitrary content at its natural height while clipping to the owner's box.",
                            index + 1,
                        ))
                    })),
            );

        let list = ScrollList::new(
            "scroll-list",
            ROW_COUNT,
            &self.list_handle,
            |range, _, _| {
                range
                    .map(|index| {
                        ListItem::new(("scroll-row", index))
                            .inset(true)
                            .child(Label::new(format!("Row {index}")))
                            .into_any_element()
                    })
                    .collect()
            },
        )
        .w_full()
        .h(px(300.))
        .border_1()
        .border_color(palette.border);

        v_flex()
            .size_full()
            .p(rems(1.))
            .gap(rems(0.75))
            .bg(palette.background)
            .child(
                h_flex()
                    .items_start()
                    .w_full()
                    .gap(rems(1.))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(super::group("Area", area)),
                    )
                    .child(
                        v_flex().flex_1().min_w_0().child(super::group(
                            "List",
                            v_flex()
                                .w_full()
                                .gap(rems(0.5))
                                .child(
                                    h_flex().child(
                                        Button::new("go-to-500", "go to 500")
                                            .style(ButtonStyle::Outlined)
                                            .on_click(move |_, cx| {
                                                let _ = page.update(cx, |this, cx| {
                                                    this.list_handle
                                                        .scroll_to_item(500, ScrollStrategy::Top);
                                                    tracing::info!(
                                                        row = 500,
                                                        "scrolled list to item"
                                                    );
                                                    cx.notify();
                                                });
                                            }),
                                    ),
                                )
                                .child(list),
                        )),
                    ),
            )
    }
}
