use gpui::{App, AppContext, Context, IntoElement, ParentElement, Render, Styled, Window, div};
use riso_ui::dev::{ThemeAppearance, choose_theme, theme_list};
use riso_ui::{Label, LabelSize, List, ListHeader, ListItem, TextColor};

pub fn init(cx: &mut App, ext: &riso_shell::Extension) {
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
        let active = riso_ui::active_theme(cx);
        let list =
            List::new()
                .header(ListHeader::new("Themes"))
                .children(theme_list(cx).into_iter().map(|entry| {
                    let appearance = match entry.appearance {
                        ThemeAppearance::Light => "light",
                        ThemeAppearance::Dark => "dark",
                    };
                    let name = entry.name;
                    ListItem::new(name.clone())
                        .toggle_state(name.as_ref() == active)
                        .child(Label::new(name.clone()))
                        .end_slot(
                            Label::new(appearance)
                                .size(LabelSize::Small)
                                .color(TextColor::Muted),
                        )
                        .on_click(move |_, cx| choose_theme(cx, &name))
                }));
        let palette = riso_ui::palette(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .p_2()
            .gap_1()
            .font(riso_ui::ui_font(cx))
            .text_size(riso_ui::ui_font_size(cx))
            .text_color(palette.text)
            .bg(palette.background)
            .border_1()
            .border_color(palette.border)
            .child(list)
    }
}
