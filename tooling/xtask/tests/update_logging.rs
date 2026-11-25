#![cfg(feature = "dev")]
//! `update` runs inside the group `yard-update{remote, ref}` (with credentials
//! stripped from `remote`): a `debug` line per step (`checked`, `fetched`,
//! `base`, `branched`, `merged`, `manifest`, `lock`, `verified`) in order on
//! success, `refused: <reason>` at `INFO` with no `ERROR` on a refusal, a
//! `WARN` naming the conflicting path on a conflict, and an `ERROR` in the
//! diagnostic log on a failed check.
//!
//! This test captures `tracing` output and starts `yard_log`, which installs a
//! process-wide `log` logger, so it is alone in its file and no other test in
//! this binary reaches the same log lines.

#[allow(dead_code)]
mod support;

use std::collections::BTreeMap;
use std::process::Command;
use std::sync::{Arc, Mutex};

use support::update::{UpdateFixture, UpdateRunner, git_commit_all, write_file};
use tracing::Level;
use xtask::{UpdateArgs, Updated, update};

/// A group that opened, or a line logged inside one.
#[derive(Debug)]
enum Record {
    Group {
        id: tracing::span::Id,
        name: &'static str,
        fields: Vec<(String, String)>,
    },
    Line {
        group: Option<tracing::span::Id>,
        level: Level,
        message: String,
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

#[derive(Clone, Default)]
struct Trace {
    records: Arc<Mutex<Vec<Record>>>,
    open: Arc<Mutex<Vec<tracing::span::Id>>>,
    /// Every field of every group and line, to verify credentials never leak.
    text: Arc<Mutex<String>>,
}

impl tracing::Subscriber for Trace {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        for (key, value) in &fields.0 {
            self.text
                .lock()
                .unwrap()
                .push_str(&format!("{key}={value}\n"));
        }
        let mut records = self.records.lock().unwrap();
        let id = tracing::span::Id::from_u64(records.len() as u64 + 1);
        records.push(Record::Group {
            id: id.clone(),
            name: attributes.metadata().name(),
            fields: fields.0,
        });
        id
    }
    fn record(&self, id: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let mut fields = Fields::default();
        values.record(&mut fields);
        for (key, value) in &fields.0 {
            self.text
                .lock()
                .unwrap()
                .push_str(&format!("{key}={value}\n"));
        }
        let mut records = self.records.lock().unwrap();
        for record in records.iter_mut() {
            if let Record::Group {
                id: group_id,
                fields: group_fields,
                ..
            } = record
                && group_id == id
            {
                group_fields.extend(fields.0);
                break;
            }
        }
    }
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        for (key, value) in &fields.0 {
            self.text
                .lock()
                .unwrap()
                .push_str(&format!("{key}={value}\n"));
        }
        let message = fields
            .0
            .iter()
            .find(|(name, _)| name.as_str() == "message")
            .map(|(_, value)| value.clone())
            .unwrap_or_default();
        let group = self.open.lock().unwrap().last().cloned();
        self.records.lock().unwrap().push(Record::Line {
            group,
            level: *event.metadata().level(),
            message,
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

/// Starts `yard_log` as `xtask::main` does, with the lifecycle trace's own
/// output off, so diagnostic lines (`log`) reach the test's subscriber as they
/// reach the lifecycle trace.
fn start_logs() {
    // SAFETY: the only test of this binary sets it before any thread reads
    // the environment.
    unsafe { std::env::set_var("XTASK_TEST_LIFECYCLE", "off") };
    yard_log::start(yard_product::Product::new(
        "xtask-test",
        "0.1.0",
        "dev.yard.xtask-test",
    ));
}

fn step_matches(message: &str, step: &str) -> bool {
    message == step
        || message.starts_with(&format!("{step}:"))
        || message.starts_with(&format!("{step} "))
}

#[test]
fn update_runs_inside_the_yard_update_group() {
    const CAPTURE_STDERR: &str = "XTASK_TEST_CAPTURE_UPDATE_STDERR";
    if std::env::var_os(CAPTURE_STDERR).is_none() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "update_runs_inside_the_yard_update_group",
                "--nocapture",
            ])
            .env(CAPTURE_STDERR, "1")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "{}\n{stderr}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(
            stderr
                .matches("Fetching main from https://127.0.0.1:9/x.git")
                .count(),
            3
        );
        assert!(
            !stderr.contains("user:se") && !stderr.contains("secret"),
            "{stderr}"
        );
        return;
    }
    start_logs();

    // 1. Successful update fixture.
    let ok_fixture = UpdateFixture::standard();
    let ok_args = UpdateArgs {
        reference: "main".into(),
        remote: Some(ok_fixture.yard_remote()),
        base: None,
        resume: false,
    };

    // 2. Refusal fixture (dirty working tree, refused before any fetch) with a
    //    credentialed remote URL.
    let refused_fixture = UpdateFixture::standard();
    write_file(&refused_fixture.dev.join("docs/x.md"), "dirty tree\n");
    let refused_args = UpdateArgs {
        reference: "main".into(),
        remote: Some("https://user:secret@example.invalid/yard.git".into()),
        base: None,
        resume: false,
    };

    // 3. Conflict fixture (both sides edit docs/x.md).
    let mut conflict_fixture = UpdateFixture::at_base();
    write_file(
        &conflict_fixture.dev.join("docs/x.md"),
        "# YARD Docs\n\nLine one from developer.\nLine two.\nLine three.\n",
    );
    git_commit_all(&conflict_fixture.dev, "developer edits docs/x.md");
    write_file(
        &conflict_fixture.yard.join("docs/x.md"),
        "# YARD Docs\n\nLine one from YARD.\nLine two.\nLine three.\n",
    );
    conflict_fixture.advance_yard_standard();
    let conflict_args = UpdateArgs {
        reference: "main".into(),
        remote: Some(conflict_fixture.yard_remote()),
        base: None,
        resume: false,
    };

    // 4. Failed check fixture.
    let failed_fixture = UpdateFixture::standard();
    let failed_args = UpdateArgs {
        reference: "main".into(),
        remote: Some(failed_fixture.yard_remote()),
        base: None,
        resume: false,
    };

    let malformed_remotes = [
        "https://user:se/cret@127.0.0.1:9/x.git",
        "https://user:se#cret@127.0.0.1:9/x.git",
        "https://user:se?cret@127.0.0.1:9/x.git",
    ];
    let malformed_fixture = UpdateFixture::standard();

    let trace = Trace::default();
    let (ok_result, refusal, conflict_result, failed_check, fetch_failures) =
        tracing::subscriber::with_default(trace.clone(), || {
            let ok = update(&ok_fixture.dev, &ok_args, &mut UpdateRunner::new()).unwrap();
            let refusal = update(
                &refused_fixture.dev,
                &refused_args,
                &mut UpdateRunner::new(),
            )
            .unwrap_err();
            let conflict = update(
                &conflict_fixture.dev,
                &conflict_args,
                &mut UpdateRunner::new(),
            )
            .unwrap();
            let failed = update(
                &failed_fixture.dev,
                &failed_args,
                &mut UpdateRunner::failing_check(),
            )
            .unwrap_err();
            let fetch_failures = malformed_remotes.map(|remote| {
                let args = UpdateArgs {
                    remote: Some(remote.into()),
                    ..UpdateArgs::default()
                };
                update(&malformed_fixture.dev, &args, &mut UpdateRunner::new()).unwrap_err()
            });
            (ok, refusal, conflict, failed, fetch_failures)
        });

    assert!(matches!(ok_result, Updated::Done { .. }));
    assert!(refusal.is_refusal());
    assert!(matches!(conflict_result, Updated::Conflicts { .. }));
    assert_eq!(failed_check.step, "verify");
    assert!(!failed_check.is_refusal());

    let records = trace.records.lock().unwrap();
    let groups: Vec<(tracing::span::Id, Vec<(String, String)>)> = records
        .iter()
        .filter_map(|record| match record {
            Record::Group { id, name, fields } if *name == "yard-update" => {
                Some((id.clone(), fields.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        groups.len(),
        7,
        "expected one yard-update group per run: {records:?}"
    );

    let field_map = |fields: &[(String, String)]| -> BTreeMap<String, String> {
        fields.iter().cloned().collect()
    };

    // Check span fields on the success run and the credentialed refusal run.
    let ok_fields = field_map(&groups[0].1);
    assert_eq!(
        ok_fields.get("remote").map(String::as_str),
        Some(ok_fixture.yard_remote().as_str()),
        "success group remote field: {ok_fields:?}"
    );
    assert_eq!(
        ok_fields.get("ref").map(String::as_str),
        Some("main"),
        "success group ref field: {ok_fields:?}"
    );

    let refused_fields = field_map(&groups[1].1);
    let refused_remote = refused_fields
        .get("remote")
        .unwrap_or_else(|| panic!("refusal group missing remote field: {refused_fields:?}"));
    assert!(
        refused_remote.contains("example.invalid/yard.git"),
        "remote field should keep the host and path: {refused_remote:?}"
    );
    assert!(
        !refused_remote.contains("secret") && !refused_remote.contains("user:"),
        "remote field must strip credentials: {refused_remote:?}"
    );
    assert_eq!(
        refused_fields.get("ref").map(String::as_str),
        Some("main"),
        "refusal group ref field: {refused_fields:?}"
    );

    let safe_remote = "https://127.0.0.1:9/x.git";
    for (failure, (_, fields)) in fetch_failures.iter().zip(&groups[4..]) {
        assert_eq!(failure.step, "fetch");
        assert_eq!(
            field_map(fields).get("remote").map(String::as_str),
            Some(safe_remote)
        );
        assert!(failure.reason.contains(safe_remote), "{failure}");
        assert!(!failure.reason.contains("user:se"), "{failure}");
    }

    type CapturedLine = (Level, String, Vec<(String, String)>);
    let lines_in = |id: &tracing::span::Id| -> Vec<CapturedLine> {
        records
            .iter()
            .filter_map(|record| match record {
                Record::Line {
                    group,
                    level,
                    message,
                    fields,
                } if group.as_ref() == Some(id) => Some((*level, message.clone(), fields.clone())),
                _ => None,
            })
            .collect()
    };

    // 1. Success run: all 8 debug steps in order, no ERROR.
    let ok_lines = lines_in(&groups[0].0);
    let debug_messages: Vec<&str> = ok_lines
        .iter()
        .filter(|(level, _, _)| *level == Level::DEBUG)
        .map(|(_, message, _)| message.as_str())
        .collect();
    let expected_steps = [
        "checked", "fetched", "base", "branched", "merged", "manifest", "lock", "verified",
    ];
    let mut cursor = 0;
    for step in expected_steps {
        let found = debug_messages[cursor..]
            .iter()
            .position(|message| step_matches(message, step))
            .unwrap_or_else(|| {
                panic!(
                    "expected DEBUG step `{step}` in order among {debug_messages:?} (all lines: {ok_lines:?})"
                )
            });
        cursor += found + 1;
    }
    assert!(
        !ok_lines.iter().any(|(level, _, _)| *level == Level::ERROR),
        "a successful run must not log an ERROR: {ok_lines:?}"
    );

    // 2. Refusal run: `refused:` at INFO inside the group, no ERROR.
    let refusal_lines = lines_in(&groups[1].0);
    assert!(
        refusal_lines
            .iter()
            .any(|(level, message, _)| *level == Level::INFO && message.starts_with("refused:")),
        "expected INFO `refused: ...` inside yard-update group: {refusal_lines:?}"
    );
    assert!(
        !refusal_lines
            .iter()
            .any(|(level, _, _)| *level == Level::ERROR),
        "a refusal must never log an ERROR: {refusal_lines:?}"
    );

    // 3. Conflict run: WARN naming the conflicting path, no ERROR.
    let conflict_lines = lines_in(&groups[2].0);
    assert!(
        conflict_lines.iter().any(|(level, message, fields)| {
            *level == Level::WARN
                && (message.contains("docs/x.md")
                    || fields.iter().any(|(_, value)| value.contains("docs/x.md")))
        }),
        "expected WARN naming docs/x.md inside yard-update group: {conflict_lines:?}"
    );
    assert!(
        !conflict_lines
            .iter()
            .any(|(level, _, _)| *level == Level::ERROR),
        "a conflict must not log an ERROR: {conflict_lines:?}"
    );

    // 4. Failed check run: ERROR in the diagnostic log inside the group.
    let failed_lines = lines_in(&groups[3].0);
    assert!(
        failed_lines
            .iter()
            .any(|(level, _, _)| *level == Level::ERROR),
        "a failed check must log an ERROR: {failed_lines:?}"
    );

    // Credentials must never appear anywhere in captured spans or events.
    let text = trace.text.lock().unwrap();
    assert!(
        !text.contains("secret") && !text.contains("user:se"),
        "credentials leaked into captured output:\n{}",
        text.lines()
            .filter(|line| line.contains("secret") || line.contains("user:se"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
