use gpui::{App, TestAppContext};
use serde_json::json;
use yard_settings::Settings;
use yard_shell::Extension;
use yard_test::{TestApp, TraceGroup, capture_trace};

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
struct AlphaSettings {
    title: String,
}

impl Settings for AlphaSettings {
    const KEY: &'static str = "alpha";
}

fn alpha_extension(cx: &mut App, extension: &Extension) {
    extension.register_settings::<AlphaSettings>(cx);
    yard_settings::observe::<AlphaSettings>(cx, |cx| {
        let title = yard_settings::get::<AlphaSettings>(cx).title.as_str();
        tracing::debug!(
            target: "harness_logging::observer",
            title,
            "alpha settings changed"
        );
    })
    .detach();
}

fn beta_extension(_cx: &mut App, _extension: &Extension) {}

fn is_in_subtree(groups: &[TraceGroup], mut group_id: Option<u64>, ancestor_id: u64) -> bool {
    while let Some(id) = group_id {
        if id == ancestor_id {
            return true;
        }
        group_id = groups
            .iter()
            .find(|group| group.id == id)
            .and_then(|group| group.parent);
    }
    false
}

#[gpui::test]
fn harness_logs_boot_and_settings_write_groups(cx: &mut TestAppContext) {
    let app = capture_trace(|trace| {
        let app = TestApp::builder()
            .extension("alpha", alpha_extension)
            .extension("beta", beta_extension)
            .settings(json!({ "alpha": { "title": "Initial" } }))
            .boot(cx);

        let boot = trace
            .group("test-app-boot")
            .expect("boot opens test-app-boot");
        assert_eq!(boot.level, tracing::Level::INFO);
        assert!(
            boot.target.starts_with("yard_test"),
            "test-app-boot target starts with yard_test: {}",
            boot.target
        );
        assert_eq!(
            boot.fields.get("extensions").map(String::as_str),
            Some("2"),
            "test-app-boot records the extension count"
        );

        let groups = trace.groups();
        let app_init = trace
            .group("app-init")
            .expect("boot runs yard_app::init inside test-app-boot");
        assert!(
            is_in_subtree(&groups, Some(app_init.id), boot.id),
            "test-app-boot surrounds app-init while boot runs"
        );

        let boot_debug_lines: Vec<_> = trace
            .lines_in(&boot)
            .into_iter()
            .filter(|line| {
                line.level == tracing::Level::DEBUG && line.target.starts_with("yard_test")
            })
            .collect();
        assert!(
            boot_debug_lines.len() >= 3,
            "test-app-boot logs debug lines for home made, settings written, and window drawn: {boot_debug_lines:#?}"
        );

        trace.assert_no_errors();
        app
    });

    capture_trace(|trace| {
        app.write_settings(cx, json!({ "alpha": { "title": "Updated" } }));

        let write = trace
            .group("test-settings-write")
            .expect("write_settings opens test-settings-write");
        assert_eq!(write.level, tracing::Level::INFO);
        assert!(
            write.target.starts_with("yard_test"),
            "test-settings-write target starts with yard_test: {}",
            write.target
        );

        let groups = trace.groups();
        let reload = trace
            .group("settings-reload")
            .expect("write_settings opens settings-reload");
        assert!(
            is_in_subtree(&groups, Some(reload.id), write.id),
            "settings-reload is under test-settings-write when boot ran in an earlier capture"
        );
        let reload_lines = trace.lines_in(&reload);
        assert!(
            reload_lines.iter().any(|line| line.message == "changed"),
            "settings-reload logs the changed section under test-settings-write: {reload_lines:#?}"
        );
        assert!(
            reload_lines.iter().any(|line| line.message == "reloaded"),
            "settings-reload logs reloaded under test-settings-write: {reload_lines:#?}"
        );
        for line in &reload_lines {
            assert!(
                is_in_subtree(&groups, line.group, write.id),
                "reload line {line:?} is under test-settings-write"
            );
        }

        let observer_line = trace
            .lines()
            .into_iter()
            .find(|line| {
                line.target == "harness_logging::observer"
                    && line.message == "alpha settings changed"
            })
            .expect("write_settings notifies the observer while captured");
        assert!(
            is_in_subtree(&groups, observer_line.group, write.id),
            "test-settings-write surrounds write_settings and its reload/observer work"
        );

        trace.assert_no_errors();
    });
}
