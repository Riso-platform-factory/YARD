use gpui::{
    App, AppContext, Context, IntoElement, ParentElement, Render, Role, Styled, Window, div,
};
use yard_ui::{Label, LabelSize, List, ListHeader, ListItem, TextColor};
use yard_ui::{ThemeAppearance, choose_theme, theme_list};

pub fn init(cx: &mut App, ext: &yard_shell::Extension) {
    let case = std::env::var("SETTINGS_TOUR").unwrap_or_default();
    if case != "themes" {
        crate::tour::init(cx, ext);
        return;
    }
    let _group = tracing::info_span!("settings-tour", case).entered();
    ext.register(cx, "settings_tour.themes", &[], |_, _, _, cx| {
        cx.new(|_| Themes).into()
    });
    ext.place_root(cx, "root", "settings_tour.themes");
}

struct Themes;

impl Render for Themes {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!("rendering themes card");
        let active = yard_ui::active_theme(cx);
        let list = List::new()
            .id("settings-tour-themes")
            .header(ListHeader::new("Themes"))
            .children(theme_list(cx).into_iter().map(|entry| {
                let appearance = match entry.appearance {
                    ThemeAppearance::Light => "light",
                    ThemeAppearance::Dark => "dark",
                };
                let name = entry.name;
                ListItem::new(name.clone())
                    .aria_role(Role::ListBoxOption)
                    .aria_label(name.clone())
                    .aria_selected(name.as_ref() == active)
                    .child(Label::new(name.clone()))
                    .end_slot(
                        Label::new(appearance)
                            .size(LabelSize::Small)
                            .color(TextColor::Muted),
                    )
                    .on_click(move |_, cx| choose_theme(cx, &name))
            }));
        let palette = yard_ui::palette(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .p_2()
            .gap_1()
            .font(yard_ui::ui_font(cx))
            .text_size(yard_ui::ui_font_size(cx))
            .text_color(palette.text)
            .bg(palette.background)
            .border_1()
            .border_color(palette.border)
            .child(list)
    }
}
