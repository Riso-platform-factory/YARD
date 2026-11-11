#[cfg(feature = "dev")]
mod dev;

fn main() {
    #[cfg(feature = "dev")]
    if dev::run() {
        return;
    }
    println!(
        "{}: no part that opens a window is in production yet.",
        env!("PRODUCT_NAME")
    );
    #[cfg(feature = "dev")]
    println!("  cargo run dev    production, with the one part under review");
}
