use gpui::{App, AppContext, Context, IntoElement, ParentElement, Render, Styled, Window, div};

pub fn init(cx: &mut App, ext: &riso_shell::Extension) {
    let case = std::env::var("THEME_TOUR").unwrap_or_default();
    if case.is_empty() {
        return;
    }
    let _group = tracing::info_span!("theme-tour", case).entered();
    match case.as_str() {
        "card" => {
            ext.register(cx, "theme_tour.card", &[], |_, _, _, cx| {
                cx.new(|_| Card).into()
            });
            ext.place_root(cx, "root", "theme_tour.card");
        }
        _ => log::warn!("unknown THEME_TOUR case {case:?}; known cases: card"),
    }
}

struct Card;

impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!("rendering card");
        let palette = riso_ui::palette(cx);
        let font = riso_ui::ui_font(cx);
        let font_size = riso_ui::ui_font_size(cx);
        let font_label = format!("{} {}", font.family, f32::from(font_size));
        let swatches = [
            ("background", palette.background),
            ("text", palette.text),
            ("border", palette.border),
            ("drop_target", palette.drop_target),
        ];
        let tabs = riso_ui::TabBar::new("theme_tour.tabs").tabs(vec![
            riso_ui::Tab::new("a", "a").selected(true),
            riso_ui::Tab::new("b", "b"),
        ]);
        div()
            .size_full()
            .flex()
            .flex_col()
            .p_2()
            .gap_1()
            .font(font)
            .text_size(font_size)
            .text_color(palette.text)
            .bg(palette.background)
            .border_1()
            .border_color(palette.border)
            .child(tabs)
            .child(riso_ui::active_theme(cx))
            .children(swatches.into_iter().map(|(name, color)| {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size_4()
                            .bg(color)
                            .border_1()
                            .border_color(palette.border),
                    )
                    .child(name)
            }))
            .child(font_label)
    }
}
