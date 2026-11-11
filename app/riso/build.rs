use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=PRODUCT_CONFIG");
    let path = env::var_os("PRODUCT_CONFIG").map_or_else(
        || PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("product.toml"),
        PathBuf::from,
    );
    println!("cargo:rerun-if-changed={}", path.display());
    let text = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("could not read product config {}: {error}", path.display())
    });
    let config: toml::Table = toml::from_str(&text)
        .unwrap_or_else(|error| panic!("invalid product config {}: {error}", path.display()));
    for (key, variable) in [("name", "PRODUCT_NAME"), ("version", "PRODUCT_VERSION")] {
        let value = config
            .get(key)
            .and_then(toml::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                panic!(
                    "product config {}: {key} must be a non-empty string",
                    path.display()
                )
            });
        assert!(
            !value.contains(['\r', '\n']),
            "product config {}: {key} must not contain line breaks",
            path.display()
        );
        println!("cargo:rustc-env={variable}={value}");
    }
}
