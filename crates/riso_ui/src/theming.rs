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

struct Provider(Font);

impl theme::ThemeSettingsProvider for Provider {
    fn ui_font<'a>(&'a self, _: &'a App) -> &'a Font {
        &self.0
    }

    fn buffer_font<'a>(&'a self, _: &'a App) -> &'a Font {
        // TODO(code-font): The code font is the UI font until a code font is bundled. Bundle Zed's Lilex when something shows code.
        &self.0
    }

    fn ui_font_size(&self, _: &App) -> Pixels {
        ui_font_size()
    }

    fn buffer_font_size(&self, _: &App) -> Pixels {
        px(15.)
    }

    fn ui_density(&self, _: &App) -> theme::UiDensity {
        theme::UiDensity::Default
    }
}

/// Initializes the bundled UI fonts and themes, and activates One Dark.
pub fn init(cx: &mut App) {
    let _span = tracing::info_span!("ui-init").entered();
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!(
            "../assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../assets/fonts/ibm-plex-sans/IBMPlexSans-Italic.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../assets/fonts/ibm-plex-sans/IBMPlexSans-SemiBold.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../assets/fonts/ibm-plex-sans/IBMPlexSans-SemiBoldItalic.ttf"
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
    theme::set_theme_settings_provider(Box::new(Provider(ui_font())), cx);
    tracing::debug!("theme settings provider installed");
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
    pub text: Hsla,
    pub border: Hsla,
    pub drop_target: Hsla,
}

pub fn palette(cx: &App) -> Palette {
    Palette {
        background: cx.theme().colors().background,
        text: cx.theme().colors().text,
        border: cx.theme().colors().border,
        drop_target: cx.theme().colors().drop_target_background,
    }
}
