use gpui::{
    App, AppContext, Context, Div, IntoElement, ParentElement, Render, Styled, Window, div, px,
    rems,
};
use riso_ui::{Headline, HeadlineSize, Label, LabelSize, TextColor, h_flex, v_flex};

const HEADLINE_SIZES: [(&str, HeadlineSize); 5] = [
    ("XLarge", HeadlineSize::XLarge),
    ("Large", HeadlineSize::Large),
    ("Medium", HeadlineSize::Medium),
    ("Small", HeadlineSize::Small),
    ("XSmall", HeadlineSize::XSmall),
];

const LABEL_SIZES: [(&str, LabelSize); 4] = [
    ("Large", LabelSize::Large),
    ("Default", LabelSize::Default),
    ("Small", LabelSize::Small),
    ("XSmall", LabelSize::XSmall),
];

const TEXT_COLORS: [(&str, TextColor); 8] = [
    ("Default", TextColor::Default),
    ("Muted", TextColor::Muted),
    ("Accent", TextColor::Accent),
    ("Disabled", TextColor::Disabled),
    ("Placeholder", TextColor::Placeholder),
    ("Error", TextColor::Error),
    ("Warning", TextColor::Warning),
    ("Success", TextColor::Success),
];

pub fn init(cx: &mut App, ext: &riso_shell::Extension) {
    let case = std::env::var("UI_GALLERY").unwrap_or_default();
    if case.is_empty() {
        return;
    }
    match case.as_str() {
        "all" => {
            let _group = tracing::info_span!("gallery-build", pages = 1).entered();
            ext.register(cx, "ui_gallery.text", &[], |_, _, _, cx| {
                cx.new(|_| TextPage).into()
            });
            ext.place_root(cx, "root", "riso.tabs");
            ext.place(cx, "text", "ui_gallery.text", "root", "children");
            tracing::debug!(page = "text", "page placed");
        }
        _ => log::warn!("unknown UI_GALLERY case {case:?}; known cases: all"),
    }
}

struct TextPage;

fn group(title: &str, palette: &riso_ui::Palette, content: impl IntoElement) -> Div {
    v_flex()
        .gap(rems(0.25))
        .child(
            h_flex()
                .gap(rems(0.75))
                .child(
                    Label::new(title.to_uppercase())
                        .size(LabelSize::XSmall)
                        .color(TextColor::Muted),
                )
                .child(div().flex_1().h(px(1.)).bg(palette.border)),
        )
        .child(content)
}

impl Render for TextPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!(page = "text", "rendering page");
        let palette = riso_ui::palette(cx);
        v_flex()
            .size_full()
            .p(rems(1.))
            .gap(rems(0.75))
            .bg(palette.background)
            .child(group(
                "Headline",
                &palette,
                v_flex().gap(rems(0.25)).children(
                    HEADLINE_SIZES
                        .into_iter()
                        .map(|(name, size)| Headline::new(name).size(size)),
                ),
            ))
            .child(group(
                "Label size",
                &palette,
                v_flex().gap(rems(0.25)).children(
                    LABEL_SIZES
                        .into_iter()
                        .map(|(name, size)| Label::new(name).size(size)),
                ),
            ))
            .child(group(
                "Label color",
                &palette,
                v_flex().gap(rems(0.25)).children(
                    TEXT_COLORS
                        .into_iter()
                        .map(|(name, color)| Label::new(name).color(color)),
                ),
            ))
            .child(group(
                "Row",
                &palette,
                h_flex()
                    .gap(rems(0.75))
                    .children(["one", "two", "three"].map(Label::new)),
            ))
            .child(group(
                "Column",
                &palette,
                v_flex()
                    .gap(rems(0.25))
                    .children(["one", "two", "three"].map(Label::new)),
            ))
    }
}
