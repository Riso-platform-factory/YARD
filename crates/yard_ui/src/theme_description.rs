use gpui::App;

/// Describes the theme section already registered by application startup.
pub fn describe_theme_settings(cx: &mut App) {
    // TODO(settings-ui): Offer theme names and fonts as schema choices. Patch the runtime-known names into the schema as enums.
    yard_settings::describe::<crate::theme_setting::ThemeSettings>(cx);
}
