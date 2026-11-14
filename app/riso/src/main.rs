#[cfg(feature = "dev")]
mod dev;
mod menus;
mod startup;

fn main() {
    #[cfg(feature = "dev")]
    if dev::run() {
        return;
    }
    startup::run(
        riso_log::Product {
            name: env!("PRODUCT_NAME"),
            version: env!("PRODUCT_VERSION"),
        },
        &[("command_palette", command_palette::init)],
        menus::menus,
    );
}
