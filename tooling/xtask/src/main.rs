//! `cargo xtask`: the alias in `.cargo/config.toml` runs this binary.

#[cfg(feature = "dev")]
fn main() -> std::process::ExitCode {
    xtask::main()
}

#[cfg(not(feature = "dev"))]
fn main() -> std::process::ExitCode {
    eprintln!("xtask: no task is in production; build with the `dev` feature");
    std::process::ExitCode::FAILURE
}
