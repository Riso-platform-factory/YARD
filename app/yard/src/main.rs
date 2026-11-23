// A production build on Windows opens no console, as Zed's does.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(feature = "dev")]
    if yard::dev::run() {
        return;
    }
    yard_app::run(
        yard::startup::application(),
        yard::startup::EXTENSIONS,
        yard::menus::menus,
    );
}
