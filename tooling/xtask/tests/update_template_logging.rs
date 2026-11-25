#![cfg(feature = "dev")]
//! Logging tests for part 2 of `upstream-updates` (template repositories):
//! `yard-update{remote, ref}` emits a `DEBUG` `base` line naming how the base
//! was found (`tree match`, `--base`, or `shared history`), a `DEBUG` `joined`
//! line when joining, and `INFO` `refused: ...` with no `ERROR` on refusal.
//!
//! This test captures `tracing` output and starts `yard_log`, which installs a
//! process-wide `log` logger, so it is alone in its file and no other test in
//! this binary reaches the same log lines.

#[allow(dead_code)]
mod support;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use support::update::{UpdateFixture, UpdateRunner, git, git_commit_all, write_file};
use tracing::Level;
use xtask::{UpdateArgs, Updated, update};

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
    },
}

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
}

impl tracing::Subscriber for Trace {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut fields = Fields::default();
        attributes.record(&mut fields);
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
        });
    }
    fn enter(&self, id: &tracing::span::Id) {
        self.open.lock().unwrap().push(id.clone());
    }
    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(self.open.lock().unwrap().pop().as_ref(), Some(id));
    }
}

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

fn find_steps_in_order(messages: &[&str], steps: &[&str], start: usize) -> usize {
    let mut cursor = start;
    for step in steps {
        let found = messages[cursor..]
            .iter()
            .position(|message| step_matches(message, step))
            .unwrap_or_else(|| {
                panic!("expected DEBUG step `{step}` after index {cursor} among {messages:?}")
            });
        cursor += found + 1;
    }
    cursor
}

#[test]
fn template_update_logs_base_and_join_inside_yard_update_group() {
    start_logs();

    // 1. Template repository matched by tree.
    let tree_fixture = UpdateFixture::from_template();
    let tree_args = UpdateArgs {
        reference: "main".into(),
        remote: Some(tree_fixture.yard_remote()),
        base: None,
        resume: false,
    };

    // 2. Template repository with explicit --base (no matching tree).
    let explicit_fixture = UpdateFixture::from_template_with_mismatched_root();
    let explicit_args = UpdateArgs {
        reference: "main".into(),
        remote: Some(explicit_fixture.yard_remote()),
        base: Some(explicit_fixture.base_commit.clone()),
        resume: false,
    };

    // 3. Refused template repository (no matching tree, no --base).
    let refused_fixture = UpdateFixture::from_template_with_mismatched_root();
    let refused_args = UpdateArgs {
        reference: "main".into(),
        remote: Some(refused_fixture.yard_remote()),
        base: None,
        resume: false,
    };

    let trace = Trace::default();
    let (tree_ok, explicit_ok, refusal, second_ok) =
        tracing::subscriber::with_default(trace.clone(), || {
            let tree_ok = update(&tree_fixture.dev, &tree_args, &mut UpdateRunner::new()).unwrap();
            let explicit_ok = update(
                &explicit_fixture.dev,
                &explicit_args,
                &mut UpdateRunner::new(),
            )
            .unwrap();
            let refusal = update(
                &refused_fixture.dev,
                &refused_args,
                &mut UpdateRunner::new(),
            )
            .unwrap_err();

            // 4. Merge first update into main, advance YARD, and run an ordinary second update.
            let Updated::Done { branch, .. } = &tree_ok else {
                panic!("expected Updated::Done on first template update");
            };
            git(&tree_fixture.dev, &["checkout", "main"]);
            git(&tree_fixture.dev, &["merge", branch]);
            write_file(
                &tree_fixture.yard.join("crates/engine/src/lib.rs"),
                "pub fn version() -> &'static str {\n    \"3\"\n}\n",
            );
            git_commit_all(&tree_fixture.yard, "yard v3");
            let second_ok =
                update(&tree_fixture.dev, &tree_args, &mut UpdateRunner::new()).unwrap();

            (tree_ok, explicit_ok, refusal, second_ok)
        });

    assert!(matches!(tree_ok, Updated::Done { .. }));
    assert!(matches!(explicit_ok, Updated::Done { .. }));
    assert!(refusal.is_refusal());
    assert!(matches!(second_ok, Updated::Done { .. }));

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
        4,
        "expected one yard-update group per run: {records:?}"
    );

    let field_map = |fields: &[(String, String)]| -> BTreeMap<String, String> {
        fields.iter().cloned().collect()
    };
    let lines_in = |id: &tracing::span::Id| -> Vec<(Level, String)> {
        records
            .iter()
            .filter_map(|record| match record {
                Record::Line {
                    group,
                    level,
                    message,
                } if group.as_ref() == Some(id) => Some((*level, message.clone())),
                _ => None,
            })
            .collect()
    };

    // 1. Tree-match run.
    let tree_fields = field_map(&groups[0].1);
    assert_eq!(
        tree_fields.get("remote").map(String::as_str),
        Some(tree_fixture.yard_remote().as_str())
    );
    assert_eq!(tree_fields.get("ref").map(String::as_str), Some("main"));
    let tree_lines = lines_in(&groups[0].0);
    let tree_debug: Vec<&str> = tree_lines
        .iter()
        .filter(|(level, _)| *level == Level::DEBUG)
        .map(|(_, msg)| msg.as_str())
        .collect();
    let after_base = find_steps_in_order(&tree_debug, &["checked", "fetched", "base"], 0);
    assert!(
        tree_debug
            .iter()
            .any(|msg| step_matches(msg, "base") && msg.contains("tree match")),
        "expected DEBUG `base` line naming `tree match`: {tree_debug:?}"
    );
    let joined_pos = tree_debug[after_base..]
        .iter()
        .position(|msg| step_matches(msg, "joined"))
        .map(|pos| after_base + pos)
        .unwrap_or_else(|| panic!("expected DEBUG `joined` after `base`: {tree_debug:?}"));
    let branched_pos = tree_debug[after_base..]
        .iter()
        .position(|msg| step_matches(msg, "branched"))
        .map(|pos| after_base + pos)
        .unwrap_or_else(|| panic!("expected DEBUG `branched` after `base`: {tree_debug:?}"));
    let after_join_branch = joined_pos.max(branched_pos) + 1;
    find_steps_in_order(
        &tree_debug,
        &["merged", "manifest", "lock", "verified"],
        after_join_branch,
    );
    assert!(
        !tree_lines.iter().any(|(level, _)| *level == Level::ERROR),
        "successful tree-match update must not log ERROR: {tree_lines:?}"
    );

    // 2. Explicit --base run.
    let explicit_lines = lines_in(&groups[1].0);
    let explicit_debug: Vec<&str> = explicit_lines
        .iter()
        .filter(|(level, _)| *level == Level::DEBUG)
        .map(|(_, msg)| msg.as_str())
        .collect();
    assert!(
        explicit_debug
            .iter()
            .any(|msg| step_matches(msg, "base") && msg.contains("--base")),
        "expected DEBUG `base` line naming `--base`: {explicit_debug:?}"
    );
    assert!(
        explicit_debug.iter().any(|msg| step_matches(msg, "joined")),
        "expected DEBUG `joined` line on --base template update: {explicit_debug:?}"
    );
    assert!(
        !explicit_lines
            .iter()
            .any(|(level, _)| *level == Level::ERROR),
        "successful --base update must not log ERROR: {explicit_lines:?}"
    );

    // 3. Refusal run.
    let refusal_lines = lines_in(&groups[2].0);
    assert!(
        refusal_lines
            .iter()
            .any(|(level, msg)| *level == Level::INFO && msg.starts_with("refused:")),
        "expected INFO `refused: ...` inside yard-update group: {refusal_lines:?}"
    );
    assert!(
        !refusal_lines
            .iter()
            .any(|(level, _)| *level == Level::ERROR),
        "a refusal must not log an ERROR: {refusal_lines:?}"
    );

    // 4. Second update after merge is ordinary (shared history, no joined).
    let second_lines = lines_in(&groups[3].0);
    let second_debug: Vec<&str> = second_lines
        .iter()
        .filter(|(level, _)| *level == Level::DEBUG)
        .map(|(_, msg)| msg.as_str())
        .collect();
    assert!(
        second_debug
            .iter()
            .any(|msg| step_matches(msg, "base") && msg.contains("shared history")),
        "expected DEBUG `base` line naming `shared history` on second update: {second_debug:?}"
    );
    assert!(
        !second_debug.iter().any(|msg| step_matches(msg, "joined")),
        "ordinary second update must not log `joined`: {second_debug:?}"
    );
    assert!(
        !second_lines.iter().any(|(level, _)| *level == Level::ERROR),
        "successful second update must not log ERROR: {second_lines:?}"
    );
}
