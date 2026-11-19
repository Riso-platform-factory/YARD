#[test]
fn lifecycle_variable_uses_the_product_environment_prefix() {
    for (name, expected) in [("yard", "YARD_LIFECYCLE"), ("my-app", "MY_APP_LIFECYCLE")] {
        let product = yard_product::Product {
            name,
            version: "1",
            identifier: "com.example.app",
        };
        assert_eq!(product.lifecycle_variable(), expected);
    }
}
