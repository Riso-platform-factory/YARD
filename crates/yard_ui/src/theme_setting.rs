use gpui::{App, Global, Window, px};
use theme::{ActiveTheme, Appearance, GlobalTheme, SystemAppearance, ThemeRegistry};

use super::theming::{
    DEFAULT_UI_FONT_FAMILY, DEFAULT_UI_FONT_SIZE, set_ui_density, set_ui_font, ui_density, ui_font,
    ui_font_size,
};

const MIN_UI_FONT_SIZE: f32 = 6.;
const MAX_UI_FONT_SIZE: f32 = 100.;

#[derive(PartialEq)]
struct ConfiguredUiFont {
    family: String,
    size: f32,
}

impl Global for ConfiguredUiFont {}

#[derive(Clone, Copy, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
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

#[derive(Clone, Copy, Default, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Density {
    Compact,
    #[default]
    Default,
    Comfortable,
}

impl std::fmt::Display for Density {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Compact => f.write_str("compact"),
            Self::Default => f.write_str("default"),
            Self::Comfortable => f.write_str("comfortable"),
        }
    }
}

impl From<Density> for theme::UiDensity {
    fn from(density: Density) -> Self {
        match density {
            Density::Compact => Self::Compact,
            Density::Default => Self::Default,
            Density::Comfortable => Self::Comfortable,
        }
    }
}

/// Theme
#[derive(serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(default)]
pub(crate) struct ThemeSettings {
    /// Follow the system appearance or always use the dark or light theme.
    pub(crate) mode: Mode,
    /// The theme used in dark mode.
    dark: String,
    /// The theme used in light mode.
    light: String,
    /// The font family used for the interface.
    ui_font_family: Option<String>,
    /// The font size used for the interface.
    ui_font_size: Option<f32>,
    /// The spacing used for the interface.
    ui_density: Option<Density>,
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self {
            mode: Mode::System,
            dark: "One Dark".into(),
            light: "One Light".into(),
            ui_font_family: None,
            ui_font_size: None,
            ui_density: None,
        }
    }
}

impl yard_settings::Settings for ThemeSettings {
    const KEY: &'static str = "theme";
}

/// Registers the `theme` section, applies the chosen theme, UI font, and density, and switches on changes.
pub fn apply_theme_setting(cx: &mut App) {
    yard_settings::register::<ThemeSettings>(cx);
    apply(cx, false);
    apply_ui_font(cx, false);
    apply_density(cx, false);
    yard_settings::observe::<ThemeSettings>(cx, |cx| {
        apply(cx, true);
        apply_ui_font(cx, true);
        apply_density(cx, true);
    })
    .detach();
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
            if matches!(yard_settings::get::<ThemeSettings>(cx).mode, Mode::System) {
                apply(cx, true);
            } else {
                tracing::debug!("mode is not system");
            }
        })
        .detach();
}

fn apply(cx: &mut App, switch: bool) {
    let settings = yard_settings::get::<ThemeSettings>(cx);
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

fn apply_ui_font(cx: &mut App, switch: bool) {
    let settings = yard_settings::get::<ThemeSettings>(cx);
    let configured_family = settings.ui_font_family.clone();
    let raw_size = settings.ui_font_size.unwrap_or(DEFAULT_UI_FONT_SIZE);
    let span_family = configured_family
        .as_deref()
        .unwrap_or(DEFAULT_UI_FONT_FAMILY);
    let _group = tracing::info_span!(
        "ui-font-apply",
        family = %span_family,
        size = %raw_size,
    )
    .entered();

    let configured = ConfiguredUiFont {
        family: span_family.to_owned(),
        size: raw_size,
    };
    if switch && cx.try_global::<ConfiguredUiFont>() == Some(&configured) {
        tracing::debug!("unchanged");
        return;
    }
    cx.set_global(configured);

    let clamped = raw_size.clamp(MIN_UI_FONT_SIZE, MAX_UI_FONT_SIZE);
    if clamped != raw_size {
        log::warn!("ui_font_size {raw_size} is clamped to {clamped}");
    }
    let size = px(clamped);

    let family = match configured_family {
        Some(family) => {
            if cx
                .text_system()
                .all_font_names()
                .iter()
                .any(|name| name == &family)
            {
                family
            } else {
                log::warn!("font {family} is not installed");
                DEFAULT_UI_FONT_FAMILY.to_owned()
            }
        }
        None => DEFAULT_UI_FONT_FAMILY.to_owned(),
    };

    if switch && ui_font(cx).family == family && ui_font_size(cx) == size {
        tracing::debug!("unchanged");
        return;
    }

    set_ui_font(cx, family, size);
    if switch {
        cx.refresh_windows();
    }
    tracing::debug!("applied");
}

fn apply_density(cx: &mut App, switch: bool) {
    let density = yard_settings::get::<ThemeSettings>(cx)
        .ui_density
        .unwrap_or_default();
    let _group = tracing::info_span!("density-apply", density = %density).entered();
    let target = theme::UiDensity::from(density);
    if switch && ui_density(cx) == target {
        tracing::debug!("unchanged");
        return;
    }
    set_ui_density(cx, target);
    if switch {
        cx.refresh_windows();
    }
    tracing::debug!("applied");
}

/// Returns the active theme's name.
pub fn active_theme(cx: &App) -> String {
    cx.theme().name.to_string()
}

#[cfg(test)]
mod tests;
