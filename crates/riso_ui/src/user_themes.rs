//! Loads user theme files in Zed's format from the user's `themes` folder into
//! the theme registry.

use gpui::App;
use std::{fs, io, path::Path};
use theme::ThemeRegistry;

use crate::theming::themes::family;

fn parse_family(bytes: &[u8]) -> Result<Option<theme::ThemeFamily>, serde_json::Error> {
    serde_json::from_slice::<serde_json::Value>(bytes)?;
    Ok(family(bytes))
}

/// Registers every theme file of `dir`. Returns how many themes were registered.
pub fn load_user_themes(cx: &mut App, dir: &Path) -> usize {
    let _group = tracing::info_span!("themes-load", path = %dir.display()).entered();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            tracing::debug!("no themes folder");
            return 0;
        }
        Err(error) => {
            log::warn!(
                "the themes folder {} cannot be read: {error}",
                dir.display()
            );
            return 0;
        }
    };

    let mut files = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.ends_with(".json") && !path.is_dir() {
                    files.push((name, path));
                }
            }
            Err(error) => {
                log::warn!("a theme file in {} cannot be read: {error}", dir.display());
            }
        }
    }
    files.sort_by(|(left, _), (right, _)| left.cmp(right));

    let registry = ThemeRegistry::global(cx);
    let mut count = 0;
    for (file, path) in files {
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                log::warn!("the theme file {file} cannot be read: {error}");
                continue;
            }
        };
        let parsed = match parse_family(&bytes) {
            Ok(parsed) => parsed,
            Err(error) => {
                log::warn!("the theme file {file} cannot be read: {error}");
                continue;
            }
        };
        let Some(family) = parsed else {
            log::warn!("the theme file {file} cannot be read: not a theme family in Zed's format");
            continue;
        };
        let themes = family.themes.len();
        registry.insert_theme_families([family]);
        count += themes;
        tracing::debug!(file, themes, "theme file loaded");
    }
    tracing::debug!(count, "user themes loaded");
    count
}
