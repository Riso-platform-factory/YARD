use gpui::App;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use yard_shell::Extension;

#[derive(Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum SortOrder {
    #[default]
    Name,
    Date,
}

#[derive(Default, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct Advanced {
    flag: bool,
}

/// Settings editor tour
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct TourSettings {
    /// Show the preview pane.
    preview: bool,
    /// The order in which items appear.
    sort: SortOrder,
    /// Items to show per page.
    #[schemars(range(min = 1, max = 100))]
    page_size: u32,
    /// The greeting shown to the user.
    greeting: String,
    /// Options kept in the settings file.
    advanced: Advanced,
}

impl Default for TourSettings {
    fn default() -> Self {
        Self {
            preview: false,
            sort: SortOrder::Name,
            page_size: 10,
            greeting: "Hello".into(),
            advanced: Advanced::default(),
        }
    }
}

impl yard_settings::Settings for TourSettings {
    const KEY: &'static str = "settings_editor_tour";
}

pub fn init(cx: &mut App, ext: &Extension) {
    if std::env::var("SETTINGS_EDITOR_TOUR").as_deref() == Ok("1") {
        let _group = tracing::info_span!("settings-editor-tour").entered();
        ext.register_editable_settings::<TourSettings>(cx);
    }
}
