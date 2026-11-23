#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod menus;

const EXTENSIONS: &[(&str, yard_app::Init)] = &[("command_palette", command_palette::init)];

fn main() {
    yard_app::run(yard_app::application!(), EXTENSIONS, menus::menus);
}
