#![cfg(feature = "dev")]

#[test]
fn engines_derive_the_same_environment_prefix() {
    for name in ["riso", "my-app 2", "Ünï"] {
        let home = riso_paths::home_variable(name);
        let lifecycle = riso_log::Product { name, version: "0" }.lifecycle_variable();
        assert_eq!(
            home.strip_suffix("_HOME").unwrap(),
            lifecycle.strip_suffix("_LIFECYCLE").unwrap(),
            "{name}"
        );
    }
}
