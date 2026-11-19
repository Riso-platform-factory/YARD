use gpui::{App, SharedString};
use theme::{Appearance, SystemAppearance, ThemeRegistry};

use crate::theme_setting::{Mode, ThemeSettings};

/// Whether a registered theme is light or dark.
pub enum ThemeAppearance {
    Light,
    Dark,
}

/// A registered theme's name and appearance.
pub struct ThemeEntry {
    pub name: SharedString,
    pub appearance: ThemeAppearance,
}

/// Every registered theme: dark first, then light, each group by name.
pub fn theme_list(cx: &App) -> Vec<ThemeEntry> {
    let mut themes = ThemeRegistry::global(cx).list();
    themes.sort_unstable_by(|a, b| {
        a.appearance
            .is_light()
            .cmp(&b.appearance.is_light())
            .then(a.name.cmp(&b.name))
    });
    themes
        .into_iter()
        .map(|theme| ThemeEntry {
            name: theme.name,
            appearance: match theme.appearance {
                Appearance::Light => ThemeAppearance::Light,
                Appearance::Dark => ThemeAppearance::Dark,
            },
        })
        .collect()
}

/// Writes the theme setting so that `name` shows, as Zed's selector does.
pub fn choose_theme(cx: &mut App, name: &str) {
    let _group = tracing::info_span!("theme-choose", name).entered();
    let Ok(theme) = ThemeRegistry::global(cx).get(name) else {
        log::error!("theme {name} is not registered");
        return;
    };
    let appearance = theme.appearance();
    let field = match appearance {
        Appearance::Light => "light",
        Appearance::Dark => "dark",
    };
    let keep_system = matches!(yard_settings::get::<ThemeSettings>(cx).mode, Mode::System)
        && appearance == SystemAppearance::global(cx).0;
    let mode = if keep_system { "system" } else { field };
    yard_settings::write::<ThemeSettings>(cx, field, serde_json::json!(name));
    if serde_json::json!(yard_settings::get::<ThemeSettings>(cx))[field] != name {
        return;
    }
    if !keep_system {
        yard_settings::write::<ThemeSettings>(cx, "mode", serde_json::json!(mode));
        if yard_settings::get::<ThemeSettings>(cx).mode.to_string() != mode {
            return;
        }
    }
    tracing::debug!(field, mode, "theme chosen");
}
