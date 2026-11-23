mod extension;
mod menus;

pub use menus::menus;

pub fn application() -> yard_app::Application {
    yard_app::application!()
}

pub const EXTENSIONS: &[(&str, yard_app::Init)] = &[
    ("command_palette", command_palette::init),
    ("north_star", extension::init),
];
