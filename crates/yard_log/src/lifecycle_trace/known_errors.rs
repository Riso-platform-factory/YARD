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
    /// Allowed log sources. An empty slice accepts any source.
    pub sources: &'static [Source],
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

pub(crate) struct Source {
    /// Inclusive range of source lines.
    pub lines: (u32, u32),
    /// Required message prefix, when specified.
    pub message_starts_with: Option<&'static str>,
}

// TODO(gpui-active-status-after-close): This entry tolerates GPUI's active-status callback for an already closed window, on macOS and on Windows. Remove it when GPUI is bumped to a revision that drops the callback with the window.
pub(crate) const KNOWN_ERRORS: &[KnownError] = &[
    KnownError {
        slug: "gpui-active-status-after-close",
        target: "gpui::window",
        message_contains: &[
            "handling active status change for window",
            "window not found",
        ],
        sources: &[],
        reason: "Both platforms run GPUI's active-status callback (gpui/src/window.rs:2181-2207) in a task, so a change reported before the window is removed can run after it, and the callback finds no window. GPUI's close callback only queues the removal (gpui/src/window.rs:1861-1881). On macOS, window_did_change_key_status queues the activate callback (gpui_macos/src/window.rs:3217 and 3287-3301). When AppKit closes the window, close_window runs the close callback and then [super close] (gpui_macos/src/window.rs:3317-3338), and the resign-key it causes queues the callback after the queued removal. When GPUI removes the window first, MacWindow::drop clears the delegate before it schedules the native close (gpui_macos/src/window.rs:1400-1436), so that close sends nothing, but a key change queued earlier still runs after the removal; the drop clears the visibility callback and not the activate one. On Windows, handle_activate_msg queues the callback for every WM_ACTIVATE (gpui_windows/src/events.rs:798-845). Drop for WindowsWindow hides the removed window, which sends WM_ACTIVATE, and drops only the visibility callback (gpui_windows/src/window.rs:613-634); a WM_ACTIVATE queued before the removal gives the same line. Zed closes its windows the same way and gets the same line. Fixed upstream by dropping the active-status callback with the window, or by skipping it for a closed window.",
        tracking: None,
    },
    // TODO(gpui-windows-redraw-after-destroy): This entry tolerates GPUI's vsync thread redrawing a window that was just destroyed. Remove it when gpui_windows stops tracking a window once it destroys it.
    KnownError {
        slug: "gpui-windows-redraw-after-destroy",
        target: "gpui_windows::platform",
        message_contains: &["Invalid window handle.", "0x80070578"],
        sources: &[Source {
            lines: (478, 480),
            message_starts_with: None,
        }],
        reason: "gpui_windows's vsync thread calls RedrawWindow on every window in raw_window_handles (gpui_windows/src/platform.rs:463-481). Drop for WindowsWindow destroys the window in a task (gpui_windows/src/window.rs:620-633); its WM_DESTROY handler only posts WM_GPUI_CLOSE_ONE_WINDOW (gpui_windows/src/events.rs:325-348), and the window leaves raw_window_handles when the main thread handles that message (gpui_windows/src/platform.rs:1238-1240 and 1292-1303). A vsync tick in between redraws a destroyed window, and RedrawWindow fails with ERROR_INVALID_WINDOW_HANDLE. The list is GPUI's own, so no application can close the gap; Zed has the same race. Fixed upstream by removing the window from raw_window_handles before destroying it.",
        tracking: None,
    },
    // TODO(wgpu-gl-probe-without-drm): This entry tolerates EGL failing to start where there is no DRM device. Remove it when gpui_wgpu stops asking wgpu for the GL backend whenever Vulkan is available, or lets the application choose the backends.
    KnownError {
        slug: "wgpu-egl-initialize-without-drm",
        target: "wgpu_hal::gles::egl",
        message_contains: &[
            "EGL 'eglInitialize' code 0x3001",
            "DRI2: failed to load driver",
        ],
        sources: &[],
        reason: "gpui_wgpu's WgpuContext::instance asks wgpu for Backends::VULKAN | Backends::GL. On a system without a DRM device, such as a CI runner with Xorg's dummy driver, wgpu-hal's GL backend calls eglInitialize, Mesa cannot load a DRI2 driver, and EGL reports EGL_NOT_INITIALIZED (0x3001) through wgpu-hal's EGL debug callback as an error. wgpu then chooses the Vulkan adapter (llvmpipe), which renders correctly, so nothing fails. Fixed by not probing GL when a Vulkan adapter is found, or by letting the application choose the backends.",
        tracking: None,
    },
    // TODO(x11-no-xinput-pointer): This entry tolerates an X display without a pointer device. Remove it when gpui_linux logs a display without pointers below the error level.
    KnownError {
        slug: "gpui-x11-no-xinput-pointers",
        target: "gpui_linux::linux::x11::client",
        message_contains: &["Found no xinput mouse pointers."],
        sources: &[],
        reason: "gpui_linux's X11 client lists the XInput pointer devices when it starts and logs an error when it finds none. A display without a mouse, such as Xorg's dummy driver on a CI runner, has none; the window still opens and draws, and only scrolling by pointer has no device to come from. Fixed upstream by logging this case as a warning or less.",
        tracking: None,
    },
    #[cfg(target_os = "windows")]
    GPUI_WINDOWS_TOAST_REENTRANCY,
];

// TODO(gpui-windows-toast-reentrancy): This entry tolerates GPUI reentering a borrowed App while posting or dismissing a toast. Remove it when GPUI moves the toast calls off the UI thread.
#[cfg_attr(
    not(any(test, target_os = "windows")),
    expect(
        dead_code,
        reason = "Windows registry entry exercised by tests on every platform"
    )
)]
const GPUI_WINDOWS_TOAST_REENTRANCY: KnownError = KnownError {
    slug: "gpui-windows-toast-reentrancy",
    target: "gpui::window",
    message_contains: &["RefCell already borrowed", "mutable App borrow"],
    sources: &[
        Source {
            lines: (1940, 1940),
            message_starts_with: Some("reading power state for frame request"),
        },
        Source {
            lines: (2052, 2052),
            message_starts_with: Some("drawing and presenting frame"),
        },
        Source {
            lines: (2077, 2077),
            message_starts_with: Some("scheduling follow-up frame"),
        },
        Source {
            lines: (2271, 2271),
            message_starts_with: Some("hit-testing window controls"),
        },
    ],
    reason: "gpui_windows calls WinRT CreateToastNotifierWithId, ToastNotifier::Show, and ToastNotifier::Hide synchronously (gpui_windows/src/system_notifications.rs) on the UI thread initialized with OleInitialize (gpui_windows/src/platform.rs). COM dispatches WM_PAINT and WM_NCHITTEST in its modal loop (gpui_windows/src/events.rs) while the App is already borrowed. The reentrant frame and window-control callbacks in gpui/src/window.rs cannot borrow the App again, so frames are dropped and hit-testing fails. Fixed upstream by moving the toast calls to a worker thread.",
    tracking: Some("zed-industries/zed#65799; Riso-platform-factory/YARD#1"),
};

pub(crate) fn find<'a>(
    list: &'a [KnownError],
    target: &str,
    line: Option<u32>,
    message: &str,
) -> Option<&'a KnownError> {
    list.iter().find(|error| {
        error.target == target
            && error.matches_source(line, message)
            && error
                .message_contains
                .iter()
                .all(|part| message.contains(part))
    })
}

impl KnownError {
    fn matches_source(&self, line: Option<u32>, message: &str) -> bool {
        self.sources.is_empty()
            || line.is_some_and(|line| {
                self.sources.iter().any(|source| {
                    let (first, last) = source.lines;
                    (first..=last).contains(&line)
                        && source
                            .message_starts_with
                            .is_none_or(|prefix| message.starts_with(prefix))
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_only_the_specific_target_and_all_substrings() {
        let message = "handling active status change for window WindowId(1v1): window not found";
        assert_eq!(
            find(KNOWN_ERRORS, "gpui::window", None, message)
                .unwrap()
                .slug,
            "gpui-active-status-after-close"
        );
        assert!(find(KNOWN_ERRORS, "other", None, message).is_none());
        for message in [
            "handling active status change for window",
            "window not found",
        ] {
            assert!(find(KNOWN_ERRORS, "gpui::window", None, message).is_none());
        }
    }

    #[test]
    fn matches_a_redraw_of_a_destroyed_window_in_gpui_windows_s_platform_only() {
        let target = "gpui_windows::platform";
        let message = "Invalid window handle. (0x80070578)";
        // The RedrawWindow call of the vsync thread, logged at line 480.
        assert_eq!(
            find(KNOWN_ERRORS, target, Some(480), message).unwrap().slug,
            "gpui-windows-redraw-after-destroy"
        );
        // The same message from the cursor broadcast's PostMessageW, at line
        // 302, or from a line it does not name, is not this race.
        for line in [Some(302), Some(477), Some(481), None] {
            assert!(
                find(KNOWN_ERRORS, target, line, message).is_none(),
                "{line:?}"
            );
        }
        // Nor is the same failure elsewhere in gpui_windows.
        assert!(find(KNOWN_ERRORS, "gpui_windows::window", Some(480), message).is_none());
        for message in [
            "Invalid window handle (0x80040102)",
            "Access is denied. (0x80070005)",
            "Invalid window handle.",
        ] {
            assert!(
                find(KNOWN_ERRORS, target, Some(480), message).is_none(),
                "{message}"
            );
        }
    }

    #[test]
    fn matches_wgpu_s_egl_probe_without_a_drm_device() {
        let target = "wgpu_hal::gles::egl";
        let message = "EGL 'eglInitialize' code 0x3001: DRI2: failed to load driver";
        assert_eq!(
            find(KNOWN_ERRORS, target, None, message).unwrap().slug,
            "wgpu-egl-initialize-without-drm"
        );
        assert!(find(KNOWN_ERRORS, "wgpu_hal::gles", None, message).is_none());
        for message in [
            "EGL 'eglInitialize' code 0x3001: some other failure",
            "EGL 'eglCreateContext' code 0x3001: DRI2: failed to load driver",
            "EGL 'eglInitialize' code 0x3003: DRI2: failed to load driver",
        ] {
            assert!(
                find(KNOWN_ERRORS, target, None, message).is_none(),
                "{message}"
            );
        }
    }

    #[test]
    fn matches_an_x11_display_without_xinput_pointers() {
        let target = "gpui_linux::linux::x11::client";
        let message = "Found no xinput mouse pointers.";
        assert_eq!(
            find(KNOWN_ERRORS, target, None, message).unwrap().slug,
            "gpui-x11-no-xinput-pointers"
        );
        assert!(
            find(
                KNOWN_ERRORS,
                "gpui_linux::linux::x11::window",
                None,
                message
            )
            .is_none()
        );
        assert!(find(KNOWN_ERRORS, target, None, "Found no xinput keyboards.").is_none());
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

    const WINDOWS_TOAST_REENTRANCY_TARGET: &str = "gpui::window";
    const WINDOWS_TOAST_REENTRANCY_CASES: &[(Option<u32>, &str)] = &[
        (
            Some(1940),
            "reading power state for frame request for window WindowId(1v1): RefCell already borrowed: attempted mutable App borrow at crates\\gpui\\src\\window.rs:1933:22; existing mutable borrow group began at crates\\gpui\\src\\window.rs:2245:22",
        ),
        (
            Some(2052),
            "drawing and presenting frame for window WindowId(1v1): RefCell already borrowed: attempted mutable App borrow at crates\\gpui\\src\\window.rs:2032:30; existing mutable borrow group began at crates\\gpui\\src\\window.rs:2245:22",
        ),
        (
            Some(2077),
            "scheduling follow-up frame for window WindowId(1v1): RefCell already borrowed: attempted mutable App borrow at crates\\gpui\\src\\window.rs:2064:22; existing mutable borrow group began at crates\\gpui\\src\\window.rs:2245:22",
        ),
        (
            Some(2271),
            "hit-testing window controls for window WindowId(1v1): RefCell already borrowed: attempted mutable App borrow at crates\\gpui\\src\\window.rs:2257:22; existing mutable borrow group began at crates\\gpui\\src\\window.rs:2245:22",
        ),
    ];

    #[cfg(target_os = "windows")]
    #[test]
    fn matches_gpui_windows_toast_reentrancy_on_windows() {
        for &(line, message) in WINDOWS_TOAST_REENTRANCY_CASES {
            assert_eq!(
                find(KNOWN_ERRORS, WINDOWS_TOAST_REENTRANCY_TARGET, line, message)
                    .map(|error| error.slug),
                Some("gpui-windows-toast-reentrancy"),
                "{line:?}: {message}"
            );
        }
    }

    #[test]
    fn rejects_unrelated_refcell_errors_for_windows_toast_reentrancy() {
        let (line, valid_message) = WINDOWS_TOAST_REENTRANCY_CASES[0];
        let single_entry = &[GPUI_WINDOWS_TOAST_REENTRANCY];
        for (fixture, entries) in [
            ("KNOWN_ERRORS", KNOWN_ERRORS),
            ("single-entry", single_entry),
        ] {
            for other_target in [
                "gpui_windows::window",
                "gpui_windows::platform",
                "gpui::app",
            ] {
                assert!(
                    find(entries, other_target, line, valid_message).is_none(),
                    "{fixture}: {other_target}"
                );
            }
            for message in [
                "handling active status change for window WindowId(1v1): RefCell already borrowed: attempted mutable App borrow at crates\\gpui\\src\\window.rs:2185:22",
                "dispatching input event for window WindowId(1v1): RefCell already borrowed: attempted mutable App borrow at crates\\gpui\\src\\window.rs:2245:22",
                "RefCell already borrowed",
                "reading power state for frame request for window WindowId(1v1): RefCell already borrowed",
                "reading power state for frame request for window WindowId(1v1): attempted mutable App borrow",
                "while handling input: reading power state for frame request for window WindowId(1v1): RefCell already borrowed: attempted mutable App borrow",
            ] {
                assert!(
                    find(entries, WINDOWS_TOAST_REENTRANCY_TARGET, line, message).is_none(),
                    "{fixture}: {message}"
                );
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn does_not_match_gpui_windows_toast_reentrancy_off_windows() {
        for &(line, message) in WINDOWS_TOAST_REENTRANCY_CASES {
            for line in [line, None] {
                assert!(
                    find(KNOWN_ERRORS, WINDOWS_TOAST_REENTRANCY_TARGET, line, message).is_none(),
                    "{line:?}: {message}"
                );
            }
        }
    }

    #[test]
    fn matches_each_windows_toast_reentrancy_operation() {
        let entries = &[GPUI_WINDOWS_TOAST_REENTRANCY];
        for &(line, message) in WINDOWS_TOAST_REENTRANCY_CASES {
            assert_eq!(
                find(entries, WINDOWS_TOAST_REENTRANCY_TARGET, line, message)
                    .map(|error| error.slug),
                Some("gpui-windows-toast-reentrancy")
            );
            for wrong_line in [None, Some(line.unwrap() + 1)] {
                assert!(
                    find(
                        entries,
                        WINDOWS_TOAST_REENTRANCY_TARGET,
                        wrong_line,
                        message
                    )
                    .is_none()
                );
            }
        }
    }

    #[test]
    fn rejects_windows_toast_reentrancy_message_at_another_operations_line() {
        let entries = &[GPUI_WINDOWS_TOAST_REENTRANCY];
        for &(line, message) in WINDOWS_TOAST_REENTRANCY_CASES {
            for &(other_line, _) in WINDOWS_TOAST_REENTRANCY_CASES {
                if other_line != line {
                    assert!(
                        find(
                            entries,
                            WINDOWS_TOAST_REENTRANCY_TARGET,
                            other_line,
                            message
                        )
                        .is_none(),
                        "{other_line:?}: {message}"
                    );
                }
            }
        }
    }
}
