/// Names the parts under review, as `<crate>/<part>`.
pub(crate) const UNDER_REVIEW: &[&str] = &[];

pub(crate) fn run() -> bool {
    if std::env::args().nth(1).as_deref() != Some("dev") {
        return false;
    }
    let product = riso_log::Product {
        name: env!("PRODUCT_NAME"),
        version: env!("PRODUCT_VERSION"),
    };
    let variable = product.lifecycle_variable();
    if std::env::var_os(&variable).is_none() {
        let crates: std::collections::BTreeSet<_> = UNDER_REVIEW
            .iter()
            .filter_map(|part| part.split('/').next())
            .collect();
        let mut filter = String::from("info");
        for name in crates {
            filter.push_str(&format!(",{name}=debug"));
        }
        // SAFETY: the logger and GPUI have not started any threads yet.
        unsafe { std::env::set_var(variable, filter) };
    }
    super::startup::run(product, &[]);
    true
}
