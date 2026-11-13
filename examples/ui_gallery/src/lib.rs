use gpui::{
    App, AppContext, Context, Div, IntoElement, ParentElement, Render, Styled, Window, div, px,
    rems,
};
use riso_ui::{
    Button, ButtonSize, ButtonStyle, Headline, HeadlineSize, Icon, IconButton, IconButtonShape,
    IconName, IconSize, Label, LabelSize, TextColor, TintColor, h_flex, v_flex,
};

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

const ICON_SIZES: [IconSize; 5] = [
    IconSize::Indicator,
    IconSize::XSmall,
    IconSize::Small,
    IconSize::Medium,
    IconSize::XLarge,
];

pub fn init(cx: &mut App, ext: &riso_shell::Extension) {
    let case = std::env::var("UI_GALLERY").unwrap_or_default();
    if case.is_empty() {
        return;
    }
    match case.as_str() {
        "all" => {
            let _group = tracing::info_span!("gallery-build", pages = 3).entered();
            ext.register(cx, "ui_gallery.text", &[], |_, _, _, cx| {
                cx.new(|_| TextPage).into()
            });
            ext.register(cx, "ui_gallery.icon", &[], |_, _, _, cx| {
                cx.new(|_| IconPage).into()
            });
            ext.register(cx, "ui_gallery.button", &[], |_, _, _, cx| {
                cx.new(|_| ButtonPage {
                    clicks: 0,
                    toggled: false,
                })
                .into()
            });
            ext.place_root(cx, "root", "riso.tabs");
            ext.place(cx, "text", "ui_gallery.text", "root", "children");
            tracing::debug!(page = "text", "page placed");
            ext.place(cx, "icon", "ui_gallery.icon", "root", "children");
            tracing::debug!(page = "icon", "page placed");
            ext.place(cx, "button", "ui_gallery.button", "root", "children");
            tracing::debug!(page = "button", "page placed");
        }
        _ => log::warn!("unknown UI_GALLERY case {case:?}; known cases: all"),
    }
}

struct TextPage;

struct IconPage;

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

impl Render for IconPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!(page = "icon", "rendering page");
        let palette = riso_ui::palette(cx);
        v_flex()
            .size_full()
            .p(rems(1.))
            .gap(rems(0.75))
            .bg(palette.background)
            .child(group(
                "Size",
                &palette,
                v_flex()
                    .gap(rems(0.25))
                    .children(ICON_SIZES.into_iter().map(|size| {
                        h_flex()
                            .gap(rems(0.5))
                            .children(IconName::ALL.iter().map(|&name| Icon::new(name).size(size)))
                    })),
            ))
            .child(group(
                "Color",
                &palette,
                h_flex().gap(rems(0.5)).children(
                    TEXT_COLORS
                        .into_iter()
                        .map(|(_, color)| Icon::new(IconName::Check).color(color)),
                ),
            ))
            .child(group(
                "Name",
                &palette,
                v_flex()
                    .gap(rems(0.25))
                    .children(IconName::ALL.iter().map(|&name| {
                        h_flex()
                            .gap(rems(0.5))
                            .child(Icon::new(name))
                            .child(Label::new(format!("{name:?}")))
                    })),
            ))
    }
}

struct ButtonPage {
    clicks: usize,
    toggled: bool,
}

impl Render for ButtonPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!(page = "button", "rendering page");
        let palette = riso_ui::palette(cx);
        let counter = cx.weak_entity();
        let toggle = cx.weak_entity();
        v_flex()
            .size_full()
            .p(rems(1.))
            .gap(rems(0.75))
            .bg(palette.background)
            .child(group(
                "Style",
                &palette,
                h_flex().flex_wrap().gap(rems(0.5)).children(
                    [
                        ButtonStyle::Filled,
                        ButtonStyle::Tinted(TintColor::Accent),
                        ButtonStyle::Tinted(TintColor::Error),
                        ButtonStyle::Tinted(TintColor::Warning),
                        ButtonStyle::Tinted(TintColor::Success),
                        ButtonStyle::Outlined,
                        ButtonStyle::Subtle,
                        ButtonStyle::Transparent,
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(index, style)| {
                        Button::new(("style", index), format!("{style:?}")).style(style)
                    }),
                ),
            ))
            .child(group(
                "Size",
                &palette,
                h_flex().flex_wrap().gap(rems(0.5)).children(
                    [
                        ButtonSize::Large,
                        ButtonSize::Medium,
                        ButtonSize::Default,
                        ButtonSize::Compact,
                        ButtonSize::None,
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(index, size)| {
                        Button::new(("size", index), format!("{size:?}")).size(size)
                    }),
                ),
            ))
            .child(group(
                "Icon",
                &palette,
                h_flex()
                    .flex_wrap()
                    .gap(rems(0.5))
                    .child(Button::new("start", "Start icon").start_icon(IconName::Check))
                    .child(Button::new("end", "End icon").end_icon(IconName::ChevronDown))
                    .child(
                        Button::new("both", "Both icons")
                            .start_icon(IconName::Check)
                            .end_icon(IconName::ChevronDown),
                    )
                    .child(IconButton::new("square", IconName::Plus).shape(IconButtonShape::Square))
                    .child(IconButton::new("wide", IconName::Plus).shape(IconButtonShape::Wide)),
            ))
            .child(group(
                "State",
                &palette,
                h_flex()
                    .flex_wrap()
                    .gap(rems(0.5))
                    .child(Button::new("disabled", "Disabled").disabled(true))
                    .child(IconButton::new("disabled-icon", IconName::Close).disabled(true))
                    .child(Button::new("toggled", "Toggled").toggle_state(true))
                    .child(IconButton::new("toggled-icon", IconName::Check).toggle_state(true)),
            ))
            .child(group(
                "Click",
                &palette,
                h_flex()
                    .gap(rems(0.5))
                    .child(
                        Button::new("count-clicks", format!("clicked {} times", self.clicks))
                            .on_click(move |_, cx| {
                                let _ = counter.update(cx, |this, cx| {
                                    this.clicks += 1;
                                    tracing::info!(count = this.clicks, "button clicked");
                                    cx.notify();
                                });
                            }),
                    )
                    .child(
                        IconButton::new("toggle-clicks", IconName::Check)
                            .toggle_state(self.toggled)
                            .on_click(move |_, cx| {
                                let _ = toggle.update(cx, |this, cx| {
                                    this.toggled = !this.toggled;
                                    tracing::info!(state = this.toggled, "icon button toggled");
                                    cx.notify();
                                });
                            }),
                    ),
            ))
    }
}
