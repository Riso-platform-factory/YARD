//! `build_script::directives` runs inside the group
//! `build-script{config, target_os}`: a `debug` line per file written, and
//! nothing at `ERROR` for a good config (nor from this module for a refused
//! config).
//!
//! This test captures `tracing` output and is alone in its file, so no other
//! test in this binary reaches the same log lines.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use yard_build_config::build_script;

/// A folder of its own under the system's temporary folder, removed when it
/// drops.
struct TempDir(PathBuf);
impl TempDir {
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn tempdir() -> TempDir {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "yard_build_config-build-script-logging-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    TempDir(path)
}

/// A group that opened, or a line logged inside one.
#[derive(Debug)]
enum Record {
    Group {
        id: tracing::span::Id,
        parent: Option<tracing::span::Id>,
        level: tracing::Level,
        name: &'static str,
        fields: Vec<(String, String)>,
    },
    Line {
        group: Option<tracing::span::Id>,
        level: tracing::Level,
        fields: Vec<(String, String)>,
    },
}

/// The fields of a span or an event as `name=value` pairs. A `?`-formatted
/// string's quotes are stripped so `name`, `%name`, and `?name` all read as
/// the bare value.
#[derive(Default)]
struct Fields(Vec<(String, String)>);
impl tracing::field::Visit for Fields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.push((field.name().into(), value.into()));
    }
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let debug = format!("{value:?}");
        let value = debug
            .strip_prefix('"')
            .and_then(|inner| inner.strip_suffix('"'))
            .unwrap_or(debug.as_str());
        self.0.push((field.name().into(), value.into()));
    }
}

/// The groups that opened and the lines logged, in order; each line keeps the
/// innermost group it was logged in, `None` when no group is open.
#[derive(Clone, Default)]
struct Trace {
    records: Arc<Mutex<Vec<Record>>>,
    open: Arc<Mutex<Vec<tracing::span::Id>>>,
}
impl tracing::Subscriber for Trace {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        let parent = self.open.lock().unwrap().last().cloned();
        let mut records = self.records.lock().unwrap();
        let id = tracing::span::Id::from_u64(records.len() as u64 + 1);
        records.push(Record::Group {
            id: id.clone(),
            parent,
            level: *attributes.metadata().level(),
            name: attributes.metadata().name(),
            fields: fields.0,
        });
        id
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let group = self.open.lock().unwrap().last().cloned();
        self.records.lock().unwrap().push(Record::Line {
            group,
            level: *event.metadata().level(),
            fields: fields.0,
        });
    }
    fn enter(&self, id: &tracing::span::Id) {
        self.open.lock().unwrap().push(id.clone());
    }
    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(self.open.lock().unwrap().pop().as_ref(), Some(id));
    }
}

/// A config under test in `tests/configs/`.
fn under_test(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/configs")
        .join(name)
        .join("product.toml")
}

#[test]
fn directives_opens_the_build_script_group_and_logs_each_written_file_at_debug() {
    let plain = under_test("plain");
    let bad = under_test("bad-identifier");
    let targets: [(&str, &[&str]); 4] = [
        ("macos", &[]),
        ("linux", &["window_icon.png"]),
        ("freebsd", &["window_icon.png"]),
        ("windows", &["app.ico", "app.rc"]),
    ];

    let trace = Trace::default();
    let mut expected_by_target: Vec<(&str, Vec<String>)> = Vec::new();
    tracing::subscriber::with_default(trace.clone(), || {
        for (target_os, expected_files) in targets {
            let out_dir = tempdir();
            let expected_paths = expected_files
                .iter()
                .map(|file| out_dir.path().join(file).display().to_string())
                .collect();
            expected_by_target.push((target_os, expected_paths));
            build_script::directives(&plain, target_os, out_dir.path()).unwrap();
        }
        let out_dir = tempdir();
        build_script::directives(&bad, "windows", out_dir.path()).unwrap_err();
    });

    let records = trace.records.lock().unwrap();
    assert!(
        records
            .iter()
            .all(|record| !matches!(record, Record::Line { group: None, .. })),
        "every line is inside a group: {records:?}"
    );
    assert!(
        !records.iter().any(|record| matches!(
            record,
            Record::Line {
                level: tracing::Level::ERROR,
                ..
            }
        )),
        "neither a good config nor a refused config logs at ERROR: {records:?}"
    );

    let find_group = |path: &Path, target_os: &str| -> tracing::span::Id {
        let config_str = path.display().to_string();
        records
            .iter()
            .find_map(|record| match record {
                Record::Group {
                    id,
                    level,
                    name,
                    fields,
                    ..
                } if *name == "build-script" => {
                    let mut keys: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
                    keys.sort();
                    assert_eq!(
                        keys,
                        ["config", "target_os"],
                        "the `build-script` group's fields"
                    );
                    let matches = fields
                        .iter()
                        .any(|(k, v)| k == "config" && *v == config_str)
                        && fields
                            .iter()
                            .any(|(k, v)| k == "target_os" && v == target_os);
                    if matches {
                        assert_eq!(
                            *level,
                            tracing::Level::INFO,
                            "`build-script` is an info_span"
                        );
                        Some(id.clone())
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!("no `build-script` group for {path:?} on {target_os}: {records:?}")
            })
    };

    let find_child_read_group = |parent_id: &tracing::span::Id, path: &Path| -> tracing::span::Id {
        let file_str = path.display().to_string();
        records
                .iter()
                .find_map(|record| match record {
                    Record::Group {
                        id,
                        parent: Some(parent),
                        name: "build-config-read",
                        fields,
                        ..
                    } if parent == parent_id
                        && fields.iter().any(|(k, v)| k == "file" && *v == file_str) =>
                    {
                        Some(id.clone())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| {
                    panic!(
                        "no `build-config-read` child group for {path:?} inside `build-script`: {records:?}"
                    )
                })
    };

    let debug_lines_in = |id: &tracing::span::Id| -> Vec<&Vec<(String, String)>> {
        records
            .iter()
            .filter_map(|record| match record {
                Record::Line {
                    group,
                    level: tracing::Level::DEBUG,
                    fields,
                } if group.as_ref() == Some(id) => Some(fields),
                _ => None,
            })
            .collect()
    };

    for (target_os, expected_paths) in &expected_by_target {
        let group_id = find_group(&plain, target_os);
        find_child_read_group(&group_id, &plain);
        let debug_lines = debug_lines_in(&group_id);
        let logged_files: Vec<Option<&str>> = debug_lines
            .iter()
            .map(|fields| {
                fields
                    .iter()
                    .find(|(k, _)| k == "file")
                    .map(|(_, v)| v.as_str())
            })
            .collect();
        let expected: Vec<Option<&str>> = expected_paths
            .iter()
            .map(|path| Some(path.as_str()))
            .collect();
        assert_eq!(
            logged_files, expected,
            "expected debug lines naming each written file in order for {target_os}: {debug_lines:?}"
        );
    }

    let bad_group_id = find_group(&bad, "windows");
    let bad_read_id = find_child_read_group(&bad_group_id, &bad);
    assert!(
        debug_lines_in(&bad_group_id).is_empty(),
        "a refused config writes no files and logs no file-written debug lines"
    );
    let refused_info_lines: Vec<&str> = records
        .iter()
        .filter_map(|record| match record {
            Record::Line {
                group,
                level: tracing::Level::INFO,
                fields,
            } if group.as_ref() == Some(&bad_read_id) => fields
                .iter()
                .find(|(k, _)| k == "message")
                .map(|(_, v)| v.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        refused_info_lines
            .iter()
            .any(|message| message.starts_with("refused: ") && message.contains("`identifier`")),
        "reader logs the refusal at info inside the `build-script` group: {records:?}"
    );
}
