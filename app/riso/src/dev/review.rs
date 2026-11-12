/// Names the parts under review, as `<crate>/<part>`.
pub(crate) const UNDER_REVIEW: &[&str] = &["riso_ui/axis", "riso_shell/shell", "riso/startup"];

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
    super::startup::run(
        product,
        &[
            ("review", announce),
            ("layout_tour", layout_tour::init),
            ("layout_guest", layout_guest::init),
        ],
    );
    true
}

fn announce(_: &mut gpui::App, _: &riso_shell::dev::Extension) {
    let under_review = if UNDER_REVIEW.is_empty() {
        String::from("nothing")
    } else {
        UNDER_REVIEW.join(", ")
    };
    tracing::info!(under_review, "dev build");
}
