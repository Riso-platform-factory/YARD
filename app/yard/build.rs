use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=PRODUCT_CONFIG");
    let path = env::var_os("PRODUCT_CONFIG").map_or_else(
        || PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("product.toml"),
        PathBuf::from,
    );
    println!("cargo:rerun-if-changed={}", path.display());
    let config = yard_build_config::read(&path).unwrap_or_else(|error| panic!("{error}"));
    println!("cargo:rerun-if-changed={}", config.icon.display());
    println!("cargo:rustc-env=PRODUCT_NAME={}", config.name);
    println!("cargo:rustc-env=PRODUCT_VERSION={}", config.version);
    println!("cargo:rustc-env=PRODUCT_IDENTIFIER={}", config.identifier);
    println!("cargo:rustc-env=PRODUCT_URL_SCHEME={}", config.url_scheme);
    println!("cargo:rustc-env=PRODUCT_ICON={}", config.icon.display());
    println!(
        "cargo:rustc-env=PRODUCT_UPDATE_URL={}",
        config.update_url.unwrap_or_default()
    );
}
