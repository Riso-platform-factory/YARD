#[test]
fn lifecycle_variable_uses_the_product_environment_prefix() {
    for (name, expected) in [("riso", "RISO_LIFECYCLE"), ("my-app", "MY_APP_LIFECYCLE")] {
        let product = riso_product::Product {
            name,
            version: "1",
            identifier: "com.example.app",
        };
        assert_eq!(product.lifecycle_variable(), expected);
    }
}
