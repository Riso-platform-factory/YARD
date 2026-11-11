#[cfg(feature = "dev")]
mod dev;

fn main() {
    #[cfg(feature = "dev")]
    if dev::run() {
        return;
    }
    println!("riso: nothing is in production yet.");
    #[cfg(feature = "dev")]
    println!("  cargo run dev    production, with the one part under review");
}
