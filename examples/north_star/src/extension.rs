gpui::actions!(north_star, [AboutNorthStar]);

pub fn init(cx: &mut gpui::App, extension: &yard_shell::Extension) {
    extension.on_action::<AboutNorthStar>(cx, |_, _, _| {
        tracing::info!("about North Star");
    });
}
