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
    println!("riso dev: under review: {names}");
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
                    title: Some("riso dev".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Review),
        )
        .expect("riso dev window opens");
        cx.activate(true);
    });
    true
}
