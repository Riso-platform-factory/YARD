//! The settings tour extension, loaded as `settings_tour` and `settings_guest` in `cargo run dev`.
//! Without `SETTINGS_TOUR`, it contributes nothing.
//! `SETTINGS_TOUR=card cargo run dev`: a card showing the values of `settings_tour`.
//! `SETTINGS_TOUR=guest cargo run dev`: `settings_guest` declares `settings_tour` and is refused.
//! `SETTINGS_TOUR=twice cargo run dev`: the second registration of `settings_tour` is refused.
//! `SETTINGS_TOUR=write cargo run dev`: a card with buttons writing a size or a wrong type.

//! `SETTINGS_TOUR=themes cargo run dev`: a card choosing the registered theme.

mod tour;
pub use tour::guest_init;

#[cfg(feature = "dev")]
mod dev;
#[cfg(feature = "dev")]
pub use dev::init;
#[cfg(not(feature = "dev"))]
pub use tour::init;
