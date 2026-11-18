//! The product: the application's name, version, and identifier, given at
//! startup, and the environment names every engine derives from the name.

/// The application identity supplied at startup.
#[derive(Debug, Clone, Copy)]
pub struct Product {
    pub name: &'static str,
    pub version: &'static str,
    /// The reverse-domain identifier, such as `vn.risotech.riso`.
    pub identifier: &'static str,
}

impl Product {
    /// The product, after checking the identifier's form inside the group
    /// `product{name, version, identifier}`. The build refuses an identifier
    /// of the wrong form first, so one here is a bug: it is logged as an
    /// error, and the product is still returned.
    pub fn new(name: &'static str, version: &'static str, identifier: &'static str) -> Product {
        let _group = tracing::info_span!("product", name, version, identifier).entered();
        if is_identifier(identifier) {
            tracing::debug!("checked");
        } else {
            tracing::error!(
                "identifier {identifier:?} is not at least two dot-separated labels of ASCII \
                 letters, digits, and `-`, not starting or ending with `-`"
            );
        }
        Product {
            name,
            version,
            identifier,
        }
    }

    /// The prefix of the product's environment variables: the name in upper
    /// case, with every character that is not an ASCII letter or digit
    /// replaced by `_`.
    pub fn environment_prefix(&self) -> String {
        self.name
            .to_uppercase()
            .chars()
            .map(|c| {
                if c.is_ascii_uppercase() || c.is_ascii_digit() {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }

    /// The environment variable that overrides the application's directories.
    pub fn home_variable(&self) -> String {
        format!("{}_HOME", self.environment_prefix())
    }

    /// The environment variable controlling the product's lifecycle trace.
    pub fn lifecycle_variable(&self) -> String {
        format!("{}_LIFECYCLE", self.environment_prefix())
    }
}

/// At least two labels separated by dots, each of ASCII letters, digits, and
/// `-`, not starting or ending with `-`, as macOS asks of a bundle identifier.
fn is_identifier(identifier: &str) -> bool {
    let mut labels = 0;
    identifier.split('.').all(|label| {
        labels += 1;
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    }) && labels >= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn product(name: &'static str) -> Product {
        Product {
            name,
            version: "0",
            identifier: "vn.risotech.riso",
        }
    }

    #[test]
    fn names_derive_from_one_prefix() {
        for (name, prefix) in [
            ("riso", "RISO"),
            ("sample", "SAMPLE"),
            ("my-app 2", "MY_APP_2"),
            ("Sample-app 2.0", "SAMPLE_APP_2_0"),
            ("café!", "CAF__"),
            ("Ünï", "_N_"),
        ] {
            let product = product(name);
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
    fn identifiers_are_accepted_and_refused() {
        for identifier in ["vn.risotech.riso", "a.b", "com.example-app.v2", "A9.b-1"] {
            assert!(is_identifier(identifier), "{identifier}");
        }
        for identifier in [
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
        ] {
            assert!(!is_identifier(identifier), "{identifier}");
        }
    }
}
