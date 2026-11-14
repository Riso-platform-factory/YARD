//! Loads the theme files bundled with riso, which are Zed's own, into the
//! theme registry. Without them the registry has only the built-in fallback,
//! whose colors differ from the real themes.
//!
//! A theme file names its colors with dots (`editor.background`); the
//! theme's color tables name them with underscores, so a file is read into
//! them directly.

const ONE: &[u8] = include_bytes!("../../assets/themes/one/one.json");
use gpui::{App, FontStyle, FontWeight, HighlightStyle, Hsla, Refineable, Rgba};
use serde_json::{Map, Value};
use std::sync::Arc;
use theme::{
    AccentColors, Appearance, PlayerColor, PlayerColors, StatusColors, StatusColorsRefinement,
    SyntaxTheme, SystemColors, Theme, ThemeColors, ThemeColorsRefinement, ThemeFamily,
    ThemeRegistry, ThemeStyles,
};

fn color(value: &Value) -> Option<Hsla> {
    Rgba::try_from(value.as_str()?).ok().map(Into::into)
}

fn theme(content: &Value) -> Option<Theme> {
    let name = content["name"].as_str()?;
    let light = content["appearance"] == "light";
    let style = content["style"].as_object()?;
    // Only colors: a value of another kind would fail the whole table.
    let colors: Map<String, Value> = style
        .iter()
        .filter(|(_, value)| color(value).is_some())
        .map(|(key, value)| (key.replace('.', "_"), value.clone()))
        .collect();

    let mut status_refinement: StatusColorsRefinement =
        serde_json::from_value(Value::Object(colors.clone())).ok()?;
    theme::apply_status_color_defaults(&mut status_refinement);
    let mut status = if light {
        StatusColors::light()
    } else {
        StatusColors::dark()
    };
    status.refine(&status_refinement);

    let mut player = if light {
        PlayerColors::light()
    } else {
        PlayerColors::dark()
    };
    for (index, listed) in style
        .get("players")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let base = player.0.get(index).copied().unwrap_or(PlayerColor {
            cursor: Hsla::default(),
            background: Hsla::default(),
            selection: Hsla::default(),
        });
        let merged = PlayerColor {
            cursor: color(&listed["cursor"]).unwrap_or(base.cursor),
            background: color(&listed["background"]).unwrap_or(base.background),
            selection: color(&listed["selection"]).unwrap_or(base.selection),
        };
        match player.0.get_mut(index) {
            Some(existing) => *existing = merged,
            None => player.0.push(merged),
        }
    }

    let mut colors_refinement: ThemeColorsRefinement =
        serde_json::from_value(Value::Object(colors)).ok()?;
    theme::apply_theme_color_defaults(&mut colors_refinement, &player);
    let mut theme_colors = if light {
        ThemeColors::light()
    } else {
        ThemeColors::dark()
    };
    theme_colors.refine(&colors_refinement);

    let syntax = style
        .get("syntax")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(token, highlight)| {
            (
                token.clone(),
                HighlightStyle {
                    color: color(&highlight["color"]),
                    background_color: color(&highlight["background_color"]),
                    font_style: match highlight["font_style"].as_str() {
                        Some("italic") => Some(FontStyle::Italic),
                        Some("oblique") => Some(FontStyle::Oblique),
                        Some("normal") => Some(FontStyle::Normal),
                        _ => None,
                    },
                    font_weight: highlight["font_weight"]
                        .as_f64()
                        .map(|weight| FontWeight(weight as f32)),
                    ..Default::default()
                },
            )
        });

    Some(Theme {
        id: format!("bundled:{name}"),
        name: name.to_owned().into(),
        appearance: if light {
            Appearance::Light
        } else {
            Appearance::Dark
        },
        styles: ThemeStyles {
            system: SystemColors::default(),
            window_background_appearance: Default::default(),
            accents: if light {
                AccentColors::light()
            } else {
                AccentColors::dark()
            },
            colors: theme_colors,
            status,
            player,
            syntax: Arc::new(SyntaxTheme::new(syntax)),
        },
    })
}

/// Reads one theme file: a family with its themes.
pub(crate) fn family(bytes: &[u8]) -> Option<ThemeFamily> {
    let content: Value = serde_json::from_slice(bytes).ok()?;
    let name = content["name"].as_str()?;
    Some(ThemeFamily {
        id: format!("bundled:{name}"),
        name: name.to_owned().into(),
        author: content["author"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
            .into(),
        themes: content["themes"]
            .as_array()?
            .iter()
            .filter_map(theme)
            .collect(),
        scales: theme::default_color_scales(),
    })
}

/// Puts every bundled theme in the registry, replacing the fallback of the
/// same name. Returns how many themes were loaded.
pub(crate) fn load_bundled(cx: &mut App) -> usize {
    let registry = ThemeRegistry::global(cx);
    match family(ONE) {
        Some(family) => {
            let loaded = family.themes.len();
            registry.insert_theme_families([family]);
            loaded
        }
        None => {
            log::error!("could not read the bundled theme one/one.json");
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_dark_has_the_colors_of_its_file() {
        let family = family(ONE).unwrap();
        let names: Vec<_> = family
            .themes
            .iter()
            .map(|theme| theme.name.to_string())
            .collect();
        assert_eq!(names, ["One Dark", "One Light"]);
        let dark = &family.themes[0];
        let hex = |color: Hsla| {
            let rgba = Rgba::from(color);
            format!(
                "#{:02x}{:02x}{:02x}",
                (rgba.r * 255.).round() as u8,
                (rgba.g * 255.).round() as u8,
                (rgba.b * 255.).round() as u8
            )
        };
        let colors = &dark.styles.colors;
        assert_eq!(hex(colors.editor_background), "#282c33");
        assert_eq!(hex(colors.panel_background), "#2f343e");
        assert_eq!(hex(colors.background), "#3b414d");
        assert_eq!(hex(colors.title_bar_background), "#3b414d");
        assert_eq!(hex(dark.styles.status.conflict), "#dec184");
        assert_eq!(hex(dark.styles.player.0[0].cursor), "#74ade8");
        assert!(dark.styles.syntax.style_for_name("keyword").is_some());
        assert_eq!(family.themes[1].appearance, Appearance::Light);
    }

    #[test]
    fn a_file_that_is_not_a_theme_is_refused() {
        assert!(family(b"not json").is_none());
        assert!(family(br#"{"themes": []}"#).is_none());
    }
}
