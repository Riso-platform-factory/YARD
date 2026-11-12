use gpui::{App, Window};
use theme::{ActiveTheme, Appearance, GlobalTheme, SystemAppearance, ThemeRegistry};

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    System,
    Dark,
    Light,
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::System => f.write_str("system"),
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
            mode: Mode::System,
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

/// Follows `window`'s appearance, recording it and switching the theme when `mode` is `system`.
pub fn follow_appearance(window: &mut Window, _cx: &mut App) {
    window
        .observe_window_appearance(|window, cx| {
            let appearance = Appearance::from(window.appearance());
            let light_or_dark = match appearance {
                Appearance::Light => "light",
                Appearance::Dark => "dark",
            };
            let _group =
                tracing::info_span!("appearance-change", appearance = %light_or_dark).entered();
            if SystemAppearance::global(cx).0 == appearance {
                tracing::debug!("unchanged");
                return;
            }
            SystemAppearance::global_mut(cx).0 = appearance;
            if matches!(riso_settings::get::<ThemeSettings>(cx).mode, Mode::System) {
                apply(cx, true);
            } else {
                tracing::debug!("mode is not system");
            }
        })
        .detach();
}

fn apply(cx: &mut App, switch: bool) {
    let settings = riso_settings::get::<ThemeSettings>(cx);
    let mode = settings.mode;
    let name = match mode {
        Mode::System => match SystemAppearance::global(cx).0 {
            Appearance::Light => settings.light.clone(),
            Appearance::Dark => settings.dark.clone(),
        },
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
