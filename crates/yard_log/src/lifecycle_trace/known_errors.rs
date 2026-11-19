//! A known error is an ERROR that is understood but cannot be fixed in the application yet.
//! Add entries only with a specific target and message substrings. Review entries
//! whenever the Zed revision is bumped and remove them once the error no longer occurs.

pub(crate) struct KnownError {
    /// Stable identifier attached to matching lifecycle lines.
    pub slug: &'static str,
    /// Exact log target that emits the error.
    pub target: &'static str,
    /// Substrings that must all appear in the error message.
    pub message_contains: &'static [&'static str],
    /// Understood cause and what would fix the error.
    #[expect(
        dead_code,
        reason = "Registry documentation for reviewing upstream errors"
    )]
    pub reason: &'static str,
    /// Issue or change tracking the fix, when available.
    #[expect(
        dead_code,
        reason = "Registry documentation for reviewing upstream errors"
    )]
    pub tracking: Option<&'static str>,
}

// TODO(gpui-key-status-after-close): This entry tolerates GPUI callbacks for an already closed window. Remove it when GPUI is bumped to a revision with the upstream key-status fix.
pub(crate) const KNOWN_ERRORS: &[KnownError] = &[KnownError {
    slug: "gpui-macos-key-status-after-close",
    target: "gpui::window",
    message_contains: &[
        "handling active status change for window",
        "window not found",
    ],
    reason: "gpui_macos's close_window runs GPUI's close callback, which removes the window, before calling [super close]. AppKit then sends windowDidResignKey, and window_did_change_key_status runs the activate callback for the removed window. Fixed upstream by returning early from window_did_change_key_status when the window state's `closed` flag is set.",
    tracking: None,
}];

pub(crate) fn find<'a>(
    list: &'a [KnownError],
    target: &str,
    message: &str,
) -> Option<&'a KnownError> {
    list.iter().find(|error| {
        error.target == target
            && error
                .message_contains
                .iter()
                .all(|part| message.contains(part))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_only_the_specific_target_and_all_substrings() {
        let message = "handling active status change for window WindowId(1v1): window not found";
        assert_eq!(
            find(KNOWN_ERRORS, "gpui::window", message).unwrap().slug,
            "gpui-macos-key-status-after-close"
        );
        assert!(find(KNOWN_ERRORS, "other", message).is_none());
        for message in [
            "handling active status change for window",
            "window not found",
        ] {
            assert!(find(KNOWN_ERRORS, "gpui::window", message).is_none());
        }
    }

    #[test]
    fn entries_have_unique_nonempty_slugs_and_message_substrings() {
        let mut slugs = std::collections::HashSet::new();
        for error in KNOWN_ERRORS {
            assert!(!error.slug.is_empty());
            assert!(slugs.insert(error.slug));
            assert!(!error.message_contains.is_empty());
        }
    }
}
