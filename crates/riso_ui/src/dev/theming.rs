//! The theme and the UI font: Zed's One themes and IBM Plex Sans, bundled
//! with the application.
//!
//! This wraps Zed's `theme` crate at revision
//! ddb90e5e89d346f21847281727e1644b8bb2cc37, licensed GPL-3.0-or-later.
//! Copyright Zed Industries, Inc.

mod themes;

use gpui::{App, Font, Hsla, Pixels, font, px};
use std::borrow::Cow;
use theme::ActiveTheme;

// TODO(theme-settings): The theme, the UI font, and its size are fixed. Read them from the settings engine when one exists.

/// Initializes the bundled UI fonts and themes, and activates One Dark.
pub fn init(cx: &mut App) {
    let _span = tracing::info_span!("ui-init").entered();
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!(
            "../../assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../../assets/fonts/ibm-plex-sans/IBMPlexSans-Italic.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../../assets/fonts/ibm-plex-sans/IBMPlexSans-SemiBold.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../../assets/fonts/ibm-plex-sans/IBMPlexSans-SemiBoldItalic.ttf"
        )),
    ];
    match cx.text_system().add_fonts(fonts) {
        Ok(()) => tracing::debug!(
            ibm_plex_sans = cx
                .text_system()
                .all_font_names()
                .iter()
                .any(|name| name == "IBM Plex Sans"),
            "bundled fonts loaded"
        ),
        Err(error) => log::error!("could not load the bundled fonts: {error}"),
    }
    theme::init(theme::LoadThemes::JustBase, cx);
    let themes = themes::load_bundled(cx);
    tracing::debug!(themes, "bundled themes loaded");
    match theme::ThemeRegistry::global(cx).get("One Dark") {
        Ok(theme) => theme::GlobalTheme::update_theme(cx, theme),
        Err(error) => log::error!("could not activate One Dark, keeping the fallback: {error}"),
    }
    tracing::debug!(theme = %cx.theme().name, "theme active");
}

/// The font for UI text.
pub fn ui_font() -> Font {
    font("IBM Plex Sans")
}

/// The size of UI text, used as a window's rem size.
pub fn ui_font_size() -> Pixels {
    px(16.)
}

/// Colors by name, for components drawn with GPUI.
/// A color is added when something draws with it.
pub struct Palette {
    pub background: Hsla,
}

pub fn palette(cx: &App) -> Palette {
    Palette {
        background: cx.theme().colors().background,
    }
}
