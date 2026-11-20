#[cfg(not(feature = "dev"))]
#[path = "build_support.rs"]
mod build_support;

use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=PRODUCT_CONFIG");
    let path = env::var_os("PRODUCT_CONFIG").map_or_else(
        || PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("product.toml"),
        PathBuf::from,
    );
    println!("cargo:rerun-if-changed={}", path.display());
    // The dev build reads the config through `yard_build_config`, the part
    // under review; the production build keeps `build_support`.
    #[cfg(feature = "dev")]
    let (name, version, identifier, url_scheme, icon, update_url) = {
        let config = yard_build_config::read(&path).unwrap_or_else(|error| panic!("{error}"));
        (
            config.name,
            config.version,
            config.identifier,
            config.url_scheme,
            config.icon,
            config.update_url.unwrap_or_default(),
        )
    };
    #[cfg(not(feature = "dev"))]
    let (name, version, identifier, url_scheme, icon, update_url) = {
        let config =
            build_support::read_product_config(&path).unwrap_or_else(|error| panic!("{error}"));
        (
            config.name,
            config.version,
            config.identifier,
            config.url_scheme,
            config.icon,
            config.update_url,
        )
    };
    println!("cargo:rerun-if-changed={}", icon.display());
    println!("cargo:rustc-env=PRODUCT_NAME={name}");
    println!("cargo:rustc-env=PRODUCT_VERSION={version}");
    println!("cargo:rustc-env=PRODUCT_IDENTIFIER={identifier}");
    println!("cargo:rustc-env=PRODUCT_URL_SCHEME={url_scheme}");
    println!("cargo:rustc-env=PRODUCT_ICON={}", icon.display());
    println!("cargo:rustc-env=PRODUCT_UPDATE_URL={update_url}");
}
