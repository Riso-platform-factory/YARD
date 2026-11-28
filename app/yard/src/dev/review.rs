/// Names the parts under review, as `<crate>/<part>`.
pub(crate) const UNDER_REVIEW: &[&str] = &[
    "yard_app/startup",
    "yard_shell/shell",
    "yard_settings/schema",
    "yard_shell/editable_settings",
    "yard_ui/switch",
    "yard_ui/dropdown",
    "yard_ui/theme_description",
    "settings_editor/editor",
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
        ("settings_editor", settings_editor::init),
        ("settings_editor_tour", settings_editor_tour::init),
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
    yard_ui::describe_theme_settings(cx);
    yard_settings::describe::<yard_shell::yard_http::NetworkSettings>(cx);
    cx.defer(|cx| {
        if let Some(path) = yard_settings::settings_path(cx) {
            let _ =
                yard_settings::write_schema_file(cx, &path.with_file_name("settings.schema.json"));
        }
    });
}
