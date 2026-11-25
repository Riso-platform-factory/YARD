use gpui::{AppContext, PathPromptOptions, TestAppContext};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};
use yard_shell::Extension;

fn extension(cx: &mut TestAppContext) -> Extension {
    cx.update(|cx| Extension::new(cx, "tour").unwrap())
}

#[gpui::test]
fn choose_paths_delivers_a_single_file_once_after_the_answer(cx: &mut TestAppContext) {
    let ext = extension(cx);
    let received: Rc<RefCell<Vec<Option<Vec<PathBuf>>>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = received.clone();

    assert!(
        !cx.did_prompt_for_paths(),
        "no open dialog is pending before asking"
    );
    cx.update(|cx| {
        ext.choose_paths(
            PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: None,
            },
            cx,
            move |paths, _| {
                sink.borrow_mut().push(paths);
            },
        );
    });
    assert!(cx.did_prompt_for_paths(), "asking opens a path dialog");

    cx.run_until_parked();
    assert!(
        received.borrow().is_empty(),
        "the callback must not run while the dialog is still pending"
    );
    assert!(
        cx.did_prompt_for_paths(),
        "the dialog stays pending until answered"
    );

    let chosen = PathBuf::from("/tmp/notes.txt");
    let answer = chosen.clone();
    cx.simulate_path_prompt_response(move |_| Some(vec![answer]));
    cx.run_until_parked();

    assert!(
        !cx.did_prompt_for_paths(),
        "answering clears the pending path dialog"
    );
    assert_eq!(
        *received.borrow(),
        vec![Some(vec![chosen])],
        "the chosen file reaches the callback once"
    );

    cx.run_until_parked();
    assert_eq!(received.borrow().len(), 1, "the callback runs exactly once");
}

#[gpui::test]
fn choose_paths_delivers_multiple_files_in_order(cx: &mut TestAppContext) {
    let ext = extension(cx);
    let received: Rc<RefCell<Vec<Option<Vec<PathBuf>>>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = received.clone();

    cx.update(|cx| {
        ext.choose_paths(
            PathPromptOptions {
                files: true,
                directories: false,
                multiple: true,
                prompt: Some("Import".into()),
            },
            cx,
            move |paths, _| {
                sink.borrow_mut().push(paths);
            },
        );
    });
    assert!(cx.did_prompt_for_paths());
    cx.run_until_parked();
    assert!(
        received.borrow().is_empty(),
        "the callback must not run before the answer"
    );

    // Unsorted order verifies that the callback preserves GPUI's order verbatim.
    let expected = vec![
        PathBuf::from("/tmp/c.txt"),
        PathBuf::from("/tmp/a.txt"),
        PathBuf::from("/tmp/b.txt"),
    ];
    let answer = expected.clone();
    cx.simulate_path_prompt_response(move |_| Some(answer));
    cx.run_until_parked();

    assert!(!cx.did_prompt_for_paths());
    assert_eq!(
        *received.borrow(),
        vec![Some(expected)],
        "the chosen files reach the callback in order, once"
    );
}

#[gpui::test]
fn choose_paths_delivers_folders(cx: &mut TestAppContext) {
    let ext = extension(cx);

    // A single folder.
    let single: Rc<RefCell<Vec<Option<Vec<PathBuf>>>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = single.clone();
    cx.update(|cx| {
        ext.choose_paths(
            PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: None,
            },
            cx,
            move |paths, _| {
                sink.borrow_mut().push(paths);
            },
        );
    });
    assert!(cx.did_prompt_for_paths());
    let folder = PathBuf::from("/tmp/workspace");
    let answer = folder.clone();
    cx.simulate_path_prompt_response(move |_| Some(vec![answer]));
    cx.run_until_parked();
    assert!(!cx.did_prompt_for_paths());
    assert_eq!(*single.borrow(), vec![Some(vec![folder])]);

    // Several folders, preserving order.
    let many: Rc<RefCell<Vec<Option<Vec<PathBuf>>>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = many.clone();
    cx.update(|cx| {
        ext.choose_paths(
            PathPromptOptions {
                files: false,
                directories: true,
                multiple: true,
                prompt: Some("Add folders".into()),
            },
            cx,
            move |paths, _| {
                sink.borrow_mut().push(paths);
            },
        );
    });
    assert!(cx.did_prompt_for_paths());
    let folders = vec![PathBuf::from("/tmp/second"), PathBuf::from("/tmp/first")];
    let answer = folders.clone();
    cx.simulate_path_prompt_response(move |_| Some(answer));
    cx.run_until_parked();
    assert!(!cx.did_prompt_for_paths());
    assert_eq!(*many.borrow(), vec![Some(folders)]);
}

#[gpui::test]
fn choose_paths_forwards_options_to_gpui_unchanged(cx: &mut TestAppContext) {
    let ext = extension(cx);
    let cases = [
        PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Import".into()),
        },
        PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        },
        PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some("Select items".into()),
        },
        PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open one".into()),
        },
    ];

    for expected in cases {
        let passed = expected.clone();
        cx.update(|cx| {
            ext.choose_paths(passed, cx, |_, _| {});
        });
        assert!(cx.did_prompt_for_paths());

        let seen: Rc<RefCell<Option<PathPromptOptions>>> = Rc::new(RefCell::new(None));
        let capture = seen.clone();
        cx.simulate_path_prompt_response(move |options| {
            *capture.borrow_mut() = Some(options.clone());
            Some(vec![PathBuf::from("/tmp/item")])
        });
        cx.run_until_parked();

        let observed = seen
            .borrow_mut()
            .take()
            .expect("GPUI received the path prompt");
        assert_eq!(observed.files, expected.files, "files option unchanged");
        assert_eq!(
            observed.directories, expected.directories,
            "directories option unchanged"
        );
        assert_eq!(
            observed.multiple, expected.multiple,
            "multiple option unchanged"
        );
        assert_eq!(observed.prompt, expected.prompt, "prompt option unchanged");
    }
}

#[gpui::test]
fn choose_paths_cancellation_gives_none_once(cx: &mut TestAppContext) {
    let ext = extension(cx);
    let received: Rc<RefCell<Vec<Option<Vec<PathBuf>>>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = received.clone();

    cx.update(|cx| {
        ext.choose_paths(
            PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: None,
            },
            cx,
            move |paths, _| {
                sink.borrow_mut().push(paths);
            },
        );
    });
    assert!(cx.did_prompt_for_paths());
    cx.run_until_parked();
    assert!(
        received.borrow().is_empty(),
        "the callback must not run before cancellation"
    );

    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();

    assert!(!cx.did_prompt_for_paths());
    assert_eq!(
        *received.borrow(),
        vec![None],
        "cancelling delivers None once"
    );
}

#[gpui::test]
fn choose_paths_refuses_when_files_and_directories_are_both_false(cx: &mut TestAppContext) {
    let ext = extension(cx);
    let cases = [
        PathPromptOptions {
            files: false,
            directories: false,
            multiple: false,
            prompt: None,
        },
        PathPromptOptions {
            files: false,
            directories: false,
            multiple: true,
            prompt: Some("Nothing".into()),
        },
    ];

    for options in cases {
        let received: Rc<RefCell<Vec<Option<Vec<PathBuf>>>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = received.clone();

        cx.update(|cx| {
            ext.choose_paths(options, cx, move |paths, _| {
                sink.borrow_mut().push(paths);
            });
        });
        assert!(
            !cx.did_prompt_for_paths(),
            "no dialog is shown when files and directories are both false"
        );

        cx.run_until_parked();
        assert!(
            !cx.did_prompt_for_paths(),
            "still no dialog after flushing pending work"
        );
        assert_eq!(
            *received.borrow(),
            vec![None],
            "the refusal delivers None once"
        );
    }
}

#[gpui::test]
fn choose_paths_refusal_from_inside_an_entity_update_updates_the_entity_without_panicking(
    cx: &mut TestAppContext,
) {
    struct Holder {
        received: Vec<Option<Vec<PathBuf>>>,
    }

    impl Holder {
        fn open_nothing(&mut self, ext: &Extension, cx: &mut gpui::Context<Self>) {
            let this = cx.weak_entity();
            ext.choose_paths(
                PathPromptOptions {
                    files: false,
                    directories: false,
                    multiple: false,
                    prompt: None,
                },
                cx,
                move |paths, cx| {
                    this.update(cx, |holder, _| {
                        holder.received.push(paths);
                    })
                    .expect("holder entity is still alive");
                },
            );
        }
    }

    let ext = extension(cx);
    let holder = cx.new(|_| Holder {
        received: Vec::new(),
    });

    holder.update(cx, |holder, cx| {
        holder.open_nothing(&ext, cx);
    });
    assert!(
        !cx.did_prompt_for_paths(),
        "no dialog is shown when files and directories are both false"
    );

    cx.run_until_parked();
    assert!(!cx.did_prompt_for_paths());
    let received = holder.read_with(cx, |holder, _| holder.received.clone());
    assert_eq!(
        received,
        vec![None],
        "the refusal delivers None to the entity once after the update finishes"
    );
}

#[gpui::test]
fn choose_new_path_starts_in_given_directory_and_delivers_chosen_path_once(
    cx: &mut TestAppContext,
) {
    let ext = extension(cx);
    let cases = [
        (
            Path::new("/tmp/exports"),
            Some("Untitled.txt"),
            PathBuf::from("/tmp/exports/report.txt"),
        ),
        (
            Path::new("/tmp/drafts"),
            None,
            PathBuf::from("/tmp/drafts/saved.md"),
        ),
    ];

    for (directory, suggested_name, chosen) in cases {
        let received: Rc<RefCell<Vec<Option<PathBuf>>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = received.clone();

        assert!(
            !cx.did_prompt_for_new_path(),
            "no save dialog is pending before asking"
        );
        cx.update(|cx| {
            ext.choose_new_path(directory, suggested_name, cx, move |path, _| {
                sink.borrow_mut().push(path);
            });
        });
        assert!(cx.did_prompt_for_new_path(), "asking opens a save dialog");

        cx.run_until_parked();
        assert!(
            received.borrow().is_empty(),
            "the callback must not run while the save dialog is pending"
        );
        assert!(cx.did_prompt_for_new_path());

        let seen_directory: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
        let capture = seen_directory.clone();
        let answer = chosen.clone();
        cx.simulate_new_path_selection(move |dir| {
            *capture.borrow_mut() = Some(dir.to_path_buf());
            Some(answer)
        });
        cx.run_until_parked();

        assert!(!cx.did_prompt_for_new_path());
        assert_eq!(
            seen_directory.borrow().as_deref(),
            Some(directory),
            "the save dialog starts in the directory given"
        );
        assert_eq!(
            *received.borrow(),
            vec![Some(chosen)],
            "the chosen save path reaches the callback once"
        );

        cx.run_until_parked();
        assert_eq!(received.borrow().len(), 1, "the callback runs exactly once");
    }
}

#[gpui::test]
fn choose_new_path_cancellation_gives_none_once(cx: &mut TestAppContext) {
    let ext = extension(cx);
    let directory = Path::new("/tmp/exports");
    let received: Rc<RefCell<Vec<Option<PathBuf>>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = received.clone();

    cx.update(|cx| {
        ext.choose_new_path(directory, Some("draft.txt"), cx, move |path, _| {
            sink.borrow_mut().push(path);
        });
    });
    assert!(cx.did_prompt_for_new_path());
    cx.run_until_parked();
    assert!(
        received.borrow().is_empty(),
        "the callback must not run before cancellation"
    );

    let seen_directory: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
    let capture = seen_directory.clone();
    cx.simulate_new_path_selection(move |dir| {
        *capture.borrow_mut() = Some(dir.to_path_buf());
        None
    });
    cx.run_until_parked();

    assert!(!cx.did_prompt_for_new_path());
    assert_eq!(seen_directory.borrow().as_deref(), Some(directory));
    assert_eq!(
        *received.borrow(),
        vec![None],
        "cancelling a save dialog delivers None once"
    );
}
