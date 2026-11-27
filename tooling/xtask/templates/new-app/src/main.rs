#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    yard_app::run(
        {{crate}}::startup::application(),
        {{crate}}::startup::EXTENSIONS,
        {{crate}}::menus::menus,
    );
}
