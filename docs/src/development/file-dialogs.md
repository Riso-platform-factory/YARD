# file-dialogs: open and save dialogs through the extension handle

An extension that reads or writes the user's files needs the system's own dialogs to choose them: the open panel and the save panel on macOS, the file chooser portal on Linux, the common item dialogs on Windows. GPUI draws them already (`App::prompt_for_paths`, `App::prompt_for_new_path`). This slice gives them to extensions through the extension handle, the way `Extension::prompt` gives the system's questions, with the lifecycle group and the diagnostic lines every engine has.

## What an extension calls

```rust
// Choose files or folders to open.
ext.choose_paths(
    gpui::PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some("Import".into()) },
    cx,
    |paths: Option<Vec<PathBuf>>, cx| { /* … */ },
);

// Choose where to save a new file.
ext.choose_new_path(&directory, Some("Untitled.txt"), cx, |path: Option<PathBuf>, cx| { /* … */ });
```

- Both are methods of `yard_shell::Extension`, beside `prompt`. The options are GPUI's own `PathPromptOptions`; YARD adds no type of its own.
- The dialog belongs to the application, not to a window, as GPUI's does: the call takes `&mut App`.
- The callback runs once, on the main thread, with the paths chosen, or `None` when the user cancelled, the dialog went away without an answer, or the system could not show it.
- `files` and `directories` both `false` is refused before any dialog: the callback gets `None`.
- `files` and `directories` both `true`: macOS lets the user pick either. Linux and Windows cannot mix them, and GPUI then shows a folder chooser; the handle says so with a warning in the trace, and an extension that needs both asks `cx.can_select_mixed_files_and_dirs()` first.
- `choose_new_path` starts in `directory` with `suggested_name` in the name field. The system's dialog asks before replacing an existing file, as each system does. The path is returned, never created or written: the extension writes it.

## Logging

- A lifecycle group `file-dialog{extension, kind}` (`kind` is `open` or `save`; `open` adds `files`, `directories`, `multiple`) surrounds the dialog from the call until the callback has run. The waiting is spawned with `.instrument(span)`, so its lines stay in the group.
- `debug`: `shown`, then `chosen` with the number of paths and the paths themselves (the lifecycle trace is never stored).
- `info`: `cancelled` (the user's own choice), and the refusal of `files` and `directories` both `false`.
- `warn`: files and folders asked together on a system that cannot mix them.
- `error`, also in the diagnostic log: the system could not show the dialog (for example no file chooser portal on Linux), with GPUI's message. The diagnostic log never holds a chosen path. On Windows, GPUI also answers with an error when the window that owns the dialog closes before or while it is shown; that is logged as `error` too, since telling it apart would mean matching GPUI's message.

## Parts

One part, `yard_shell/file_dialog`, in `crates/yard_shell/src/dev/file_dialog.rs` (the crate gains `src/dev.rs`). It adds the two methods to `Extension` without changing the production part `yard_shell/shell`.

- **Acceptance tests** (in `crates/yard_shell/tests/`, `#![cfg(feature = "dev")]`), with GPUI's test platform (`simulate_path_prompt_response`, `simulate_new_path_selection`): the paths reach the callback; cancel gives `None`; the callback runs once; the refusal shows no dialog; the options reach GPUI unchanged; and, in their own files, the group, its fields, and the levels above.
- **Test extension** `examples/file_dialog_tour`, named in `review.rs` while the part is under review. Without `FILE_DIALOG_TOUR` it does nothing. With `FILE_DIALOG_TOUR=open`, `open-many`, `folders`, `mixed`, or `save`, a root card has a button that opens that dialog and shows what came back: the paths, `(no paths)` for an empty selection, or `cancelled` for `None`. A dialog the system could not show also gives `None`, so the card says `cancelled` and the error is in the trace and the diagnostic log.
- **Run by the lead on macOS** with every case. Linux and Windows build in the runner checks; their dialogs need a person, so they are run by hand on those systems when the user has one, and the report says which were run.

## Not in this slice

- File type filters and a starting folder for the open dialog: GPUI at the pinned revision has neither. `// TODO(file-dialogs-filters): filters and a starting folder for choose_paths. Unblocked when GPUI's PathPromptOptions has them.`
- A dialog drawn by YARD when the system has none (Zed falls back to its own path picker): the error is logged and the extension gets `None`.
- Reading or writing the chosen files, recent files, and sandbox bookmarks (`mac-app-store`).
