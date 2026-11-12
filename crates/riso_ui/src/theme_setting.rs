use gpui::App;
use theme::{ActiveTheme, GlobalTheme, ThemeRegistry};

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Dark,
    Light,
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dark => f.write_str("dark"),
            Self::Light => f.write_str("light"),
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(default)]
struct ThemeSettings {
    mode: Mode,
    dark: String,
    light: String,
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self {
            mode: Mode::Dark,
            dark: "One Dark".into(),
            light: "One Light".into(),
        }
    }
}

impl riso_settings::Settings for ThemeSettings {
    const KEY: &'static str = "theme";
}

/// Registers the `theme` section, applies the chosen theme, and switches on changes.
pub fn apply_theme_setting(cx: &mut App) {
    riso_settings::register::<ThemeSettings>(cx);
    apply(cx, false);
    riso_settings::observe::<ThemeSettings>(cx, |cx| apply(cx, true)).detach();
}

fn apply(cx: &mut App, switch: bool) {
    let settings = riso_settings::get::<ThemeSettings>(cx);
    let mode = settings.mode;
    let name = match mode {
        Mode::Dark => settings.dark.clone(),
        Mode::Light => settings.light.clone(),
    };
    let _group = tracing::info_span!("theme-apply", mode = %mode, theme = %name).entered();
    if switch && name == active_theme(cx) {
        tracing::debug!("unchanged");
        return;
    }
    match ThemeRegistry::global(cx).get(&name) {
        Ok(theme) => {
            GlobalTheme::update_theme(cx, theme);
            if switch {
                cx.refresh_windows();
            }
            tracing::debug!("applied");
        }
        Err(_) => log::warn!("theme {name} is not registered"),
    }
}

/// Returns the active theme's name.
pub fn active_theme(cx: &App) -> String {
    cx.theme().name.to_string()
}

#[cfg(test)]
mod tests;
