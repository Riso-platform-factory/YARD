/// The sample application, from the values its build script sets.
pub fn application() -> yard_app::Application {
    yard_app::application!()
}

/// The extensions that come with the application.
pub const EXTENSIONS: &[(&str, yard_app::Init)] = &[
    ("command_palette", command_palette::init),
    ("settings_editor", settings_editor::init),
];
