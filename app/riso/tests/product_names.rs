#[test]
fn engines_derive_the_same_environment_prefix() {
    for name in ["riso", "my-app 2", "Ünï"] {
        let product = riso_product::Product {
            name,
            version: "0",
            identifier: "com.example.app",
        };
        let home = product.home_variable();
        let lifecycle = product.lifecycle_variable();
        assert_eq!(
            home.strip_suffix("_HOME").unwrap(),
            lifecycle.strip_suffix("_LIFECYCLE").unwrap(),
            "{name}"
        );
    }
}
