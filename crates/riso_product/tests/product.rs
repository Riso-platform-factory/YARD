#![cfg(feature = "dev")]
//! `riso_product/product` is under review: the names derived from the product's
//! name, and the identifiers `Product::new` accepts and refuses.

#[test]
fn names_form_the_environment_prefix_home_and_lifecycle_variables() {
    for (name, prefix) in [
        ("riso", "RISO"),
        ("sample", "SAMPLE"),
        ("my-app", "MY_APP"),
        ("my-app 2", "MY_APP_2"),
        ("Sample-app 2.0", "SAMPLE_APP_2_0"),
        ("café!", "CAF__"),
        ("Ünï", "_N_"),
    ] {
        let product = riso_product::dev::Product::new(name, "1", "vn.risotech.riso");
        assert_eq!(product.environment_prefix(), prefix, "{name}");
        assert_eq!(product.home_variable(), format!("{prefix}_HOME"), "{name}");
        assert_eq!(
            product.lifecycle_variable(),
            format!("{prefix}_LIFECYCLE"),
            "{name}"
        );
    }
}

#[test]
fn new_returns_a_product_for_identifiers_the_contract_accepts_and_refuses() {
    // At least two labels separated by dots, each of ASCII letters, digits,
    // and `-`, not starting or ending with `-` — as macOS asks of a bundle
    // identifier.
    let accepted = ["vn.risotech.riso", "a.b", "com.example-app.v2", "A9.b-1"];
    // Too few labels, an empty label, a label starting or ending with `-`, or
    // a character outside the allowed set.
    let refused = [
        "",
        "sample",
        ".com.example",
        "com.example.",
        "com..example",
        "-com.example",
        "com-.example",
        "com.ex ample",
        "com.exa_mple",
        "com.exämple",
    ];
    for identifier in accepted.into_iter().chain(refused) {
        let product = riso_product::dev::Product::new("sample", "9.9.9", identifier);
        assert_eq!(product.name, "sample", "{identifier}");
        assert_eq!(product.version, "9.9.9", "{identifier}");
        assert_eq!(product.identifier, identifier, "{identifier}");
    }
}
