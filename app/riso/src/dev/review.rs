use gpui::{
    App, Context, IntoElement, Render, TitlebarOptions, Window, WindowOptions, div, prelude::*,
};

/// Names the one part under review, as `<crate>/<part>`.
pub(crate) const UNDER_REVIEW: &[&str] = &[];

struct Review;

impl Render for Review {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

pub(crate) fn run() -> bool {
    if std::env::args().nth(1).as_deref() != Some("dev") {
        return false;
    }
    let names = if UNDER_REVIEW.is_empty() {
        "nothing".to_owned()
    } else {
        UNDER_REVIEW.join(", ")
    };
    println!("{} dev: under review: {names}", env!("PRODUCT_NAME"));
    gpui_platform::application().run(|cx: &mut App| {
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(format!("{} dev", env!("PRODUCT_NAME")).into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Review),
        )
        .unwrap_or_else(|error| panic!("{} dev window opens: {error}", env!("PRODUCT_NAME")));
        cx.activate(true);
    });
    true
}
