use std::sync::Arc;
use yard_shell::yard_http::{HttpClient, NetworkSettings, user_agent};

/// Names the parts under review, as `<crate>/<part>`.
pub(crate) const UNDER_REVIEW: &[&str] = &[
    "yard_app/startup",
    "yard_http/client",
    "yard_log/diagnostic_log",
    "yard_log/lifecycle_trace",
    "yard_shell/http",
    "yard_shell/shell",
];

pub fn run() -> bool {
    if std::env::args().nth(1).as_deref() != Some("dev") {
        return false;
    }
    let app = crate::startup::application();
    let variable = app.product.lifecycle_variable();
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
    let mut extensions: Vec<(&'static str, yard_app::Init)> = vec![
        ("review", announce),
        ("toggle_host", toggle_tour::host),
        ("toggle_tour", toggle_tour::init),
        ("http_tour", http_tour::init),
    ];
    extensions.extend_from_slice(crate::startup::EXTENSIONS);
    yard_app::run(app, &extensions, crate::menus::menus);
    true
}

fn announce(cx: &mut gpui::App, _: &yard_shell::Extension) {
    let under_review = if UNDER_REVIEW.is_empty() {
        String::from("nothing")
    } else {
        UNDER_REVIEW.join(", ")
    };
    tracing::info!(under_review, "dev build");

    yard_settings::register::<NetworkSettings>(cx);
    let product = crate::startup::application().product;
    let agent = user_agent(product.name, product.version);
    let client = match HttpClient::new(&agent, yard_settings::get::<NetworkSettings>(cx)) {
        Ok(client) => client,
        Err(error) => {
            log::error!("HTTP client failed to start: {error}");
            HttpClient::unavailable()
        }
    };
    yard_shell::set_http_client(cx, client.clone());
    cx.set_http_client(Arc::new(client.for_caller("gpui")));
    yard_settings::observe::<NetworkSettings>(cx, move |cx| {
        client.reconfigure(yard_settings::get::<NetworkSettings>(cx));
    })
    .detach();
}
