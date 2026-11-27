#![cfg(feature = "dev")]

use std::{
    collections::HashSet,
    panic::{AssertUnwindSafe, catch_unwind},
};
use yard_test::capture_trace;

fn panic_text(payload: &Box<dyn std::any::Any + Send>) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("")
}

#[test]
fn captures_groups_fields_lines_levels_targets_and_membership() {
    let returned = capture_trace(|trace| {
        {
            let _outer = tracing::info_span!(
                target: "logs_trace::outer",
                "outer-work",
                count = 3,
                kind = "batch"
            )
            .entered();
            tracing::debug!(
                target: "logs_trace::outer",
                step = "prepare",
                "preparing work"
            );
            {
                let _inner = tracing::info_span!(
                    target: "logs_trace::inner",
                    "inner-step",
                    item = "alpha"
                )
                .entered();
                tracing::warn!(target: "logs_trace::inner", "inner warning");
            }
            tracing::info!(target: "logs_trace::outer", "outer finished");
        }
        tracing::trace!(target: "logs_trace::top", "top-level trace");

        let outer = trace.group("outer-work").expect("outer-work group");
        assert_eq!(outer.parent, None);
        assert_eq!(outer.level, tracing::Level::INFO);
        assert_eq!(outer.target, "logs_trace::outer");
        assert_eq!(outer.fields.get("count").map(String::as_str), Some("3"));
        assert_eq!(outer.fields.get("kind").map(String::as_str), Some("batch"));

        let inner = trace.group("inner-step").expect("inner-step group");
        assert_eq!(inner.parent, Some(outer.id));
        assert_eq!(inner.level, tracing::Level::INFO);
        assert_eq!(inner.target, "logs_trace::inner");
        assert_eq!(inner.fields.get("item").map(String::as_str), Some("alpha"));

        let outer_lines = trace.lines_in(&outer);
        assert_eq!(
            outer_lines.len(),
            2,
            "outer-work owns only its direct lines: {outer_lines:#?}"
        );
        assert_eq!(outer_lines[0].group, Some(outer.id));
        assert_eq!(outer_lines[0].level, tracing::Level::DEBUG);
        assert_eq!(outer_lines[0].target, "logs_trace::outer");
        assert_eq!(outer_lines[0].message, "preparing work");
        assert_eq!(
            outer_lines[0].fields.get("step").map(String::as_str),
            Some("prepare")
        );
        assert_eq!(outer_lines[1].group, Some(outer.id));
        assert_eq!(outer_lines[1].level, tracing::Level::INFO);
        assert_eq!(outer_lines[1].target, "logs_trace::outer");
        assert_eq!(outer_lines[1].message, "outer finished");

        let inner_lines = trace.lines_in(&inner);
        assert_eq!(inner_lines.len(), 1, "inner-step owns its line");
        assert_eq!(inner_lines[0].group, Some(inner.id));
        assert_eq!(inner_lines[0].level, tracing::Level::WARN);
        assert_eq!(inner_lines[0].target, "logs_trace::inner");
        assert_eq!(inner_lines[0].message, "inner warning");

        let top = trace
            .lines()
            .into_iter()
            .find(|line| line.message == "top-level trace")
            .expect("top-level trace line");
        assert_eq!(top.group, None);
        assert_eq!(top.level, tracing::Level::TRACE);
        assert_eq!(top.target, "logs_trace::top");

        42usize
    });

    assert_eq!(returned, 42, "capture_trace returns the closure's value");
}

#[test]
fn captures_nothing_logged_before_or_after_its_closure() {
    {
        let _before = tracing::info_span!(target: "logs_trace::outside", "before-group").entered();
        tracing::info!(target: "logs_trace::outside", "logged before capture");
    }

    capture_trace(|trace| {
        assert!(
            trace.groups().is_empty(),
            "groups opened before the closure are not captured"
        );
        assert!(
            trace.lines().is_empty(),
            "lines logged before the closure are not captured"
        );
        tracing::info!(target: "logs_trace::inside", "logged inside capture");
        assert_eq!(trace.lines().len(), 1);
        assert_eq!(trace.lines()[0].message, "logged inside capture");
    });

    {
        let _after = tracing::info_span!(target: "logs_trace::outside", "after-group").entered();
        tracing::info!(target: "logs_trace::outside", "logged after capture");
    }

    capture_trace(|trace| {
        assert!(
            trace.groups().is_empty(),
            "groups from previous captures or after the closure do not leak into the next capture"
        );
        assert!(
            trace.lines().is_empty(),
            "lines from previous captures or after the closure do not leak into the next capture"
        );
    });
}

#[test]
fn group_returns_the_first_match_and_errors_warnings_filter_by_level() {
    capture_trace(|trace| {
        {
            let _first =
                tracing::info_span!(target: "logs_trace::repeat", "repeated", which = "first")
                    .entered();
        }
        {
            let _second =
                tracing::info_span!(target: "logs_trace::repeat", "repeated", which = "second")
                    .entered();
        }

        let repeated_count = trace
            .groups()
            .into_iter()
            .filter(|group| group.name == "repeated")
            .count();
        assert_eq!(repeated_count, 2, "both repeated groups are recorded");

        let first = trace.group("repeated").expect("repeated group");
        assert_eq!(
            first.fields.get("which").map(String::as_str),
            Some("first"),
            "group(name) returns the first group with that name"
        );
        assert!(
            trace.group("does-not-exist").is_none(),
            "group(name) returns None when absent"
        );

        tracing::trace!(target: "logs_trace::levels", "trace msg");
        tracing::debug!(target: "logs_trace::levels", "debug msg");
        tracing::info!(target: "logs_trace::levels", "info msg");
        tracing::warn!(target: "logs_trace::levels", "first warning");
        tracing::error!(target: "logs_trace::levels", "first error");
        tracing::warn!(target: "logs_trace::levels", "second warning");
        tracing::error!(target: "logs_trace::levels", "second error");

        assert_eq!(
            trace.warnings(),
            vec!["first warning", "second warning"],
            "warnings() returns only WARN messages"
        );
        assert_eq!(
            trace.errors(),
            vec!["first error", "second error"],
            "errors() returns only ERROR messages"
        );
    });
}

#[test]
fn assert_no_errors_passes_without_errors_and_panics_with_an_error() {
    capture_trace(|trace| {
        tracing::info!(target: "logs_trace::assert", "all good");
        tracing::warn!(target: "logs_trace::assert", "just a warning");
        trace.assert_no_errors();
    });

    capture_trace(|trace| {
        tracing::error!(target: "logs_trace::assert", "something failed");
        let panicked = catch_unwind(AssertUnwindSafe(|| trace.assert_no_errors()))
            .expect_err("assert_no_errors must panic when an error was logged");
        assert!(
            !panic_text(&panicked).contains("not yet implemented"),
            "expected assert_no_errors panic, not todo!()"
        );
    });
}

#[test]
fn ids_are_unique_across_captures_and_foreign_explicit_parent_gives_no_parent() {
    let (first_ids, kept_span) = capture_trace(|trace| {
        let span = tracing::info_span!(target: "logs_trace::ids", "first-capture-group");
        {
            let _entered = span.enter();
            let _nested =
                tracing::info_span!(target: "logs_trace::ids", "first-capture-nested").entered();
        }
        let ids: HashSet<u64> = trace.groups().into_iter().map(|group| group.id).collect();
        assert_eq!(ids.len(), 2, "first capture recorded two groups");
        (ids, span)
    });

    capture_trace(|trace| {
        let local = tracing::info_span!(target: "logs_trace::ids", "second-capture-local");
        let _local_entered = local.enter();

        {
            let _foreign_child = tracing::info_span!(
                target: "logs_trace::ids",
                parent: &kept_span,
                "foreign-parent-group"
            )
            .entered();
        }
        tracing::info!(
            target: "logs_trace::ids",
            parent: &kept_span,
            "foreign-parent-line"
        );
        {
            let _local_child = tracing::info_span!(
                target: "logs_trace::ids",
                parent: &local,
                "local-parent-group"
            )
            .entered();
        }

        let second_ids: HashSet<u64> = trace.groups().into_iter().map(|group| group.id).collect();
        assert!(
            first_ids.is_disjoint(&second_ids),
            "groups of two captures must never share an id: first={first_ids:?} second={second_ids:?}"
        );

        let local_group = trace
            .group("second-capture-local")
            .expect("second-capture-local group");
        let foreign_child = trace
            .group("foreign-parent-group")
            .expect("foreign-parent-group");
        assert_eq!(
            foreign_child.parent, None,
            "a group whose explicit parent belongs to an earlier capture gets no parent"
        );

        let foreign_line = trace
            .lines()
            .into_iter()
            .find(|line| line.message == "foreign-parent-line")
            .expect("foreign-parent-line recorded");
        assert_eq!(
            foreign_line.group, None,
            "a line whose explicit parent belongs to an earlier capture gets no parent"
        );

        let local_child = trace
            .group("local-parent-group")
            .expect("local-parent-group");
        assert_eq!(
            local_child.parent,
            Some(local_group.id),
            "an explicit parent from the same capture still attaches"
        );
    });
}

#[test]
fn span_current_is_entered_group_and_in_scope_attaches_lines() {
    capture_trace(|trace| {
        let current = {
            let group = tracing::info_span!(target: "logs_trace::current", "current-group");
            let _entered = group.enter();
            let current = tracing::Span::current();
            assert!(
                !current.is_disabled(),
                "Span::current() inside an entered group must not be disabled"
            );
            assert_eq!(
                current.id(),
                group.id(),
                "Span::current() inside an entered group must be that group"
            );
            current
        };

        current.in_scope(|| {
            tracing::info!(
                target: "logs_trace::current",
                "logged after Span::current().in_scope"
            );
        });

        let recorded = trace.group("current-group").expect("current-group");
        let lines = trace.lines_in(&recorded);
        assert_eq!(
            lines.len(),
            1,
            "line logged in Span::current().in_scope(...) must belong to the group: {lines:#?}"
        );
        assert_eq!(lines[0].group, Some(recorded.id));
        assert_eq!(lines[0].message, "logged after Span::current().in_scope");
    });
}

#[test]
fn returned_entered_guard_does_not_panic_when_dropped_after_capture() {
    let returned_guard = capture_trace(|trace| {
        let entered = tracing::info_span!(target: "logs_trace::late", "returned-guard").entered();
        tracing::info!(target: "logs_trace::late", "inside returned guard");
        let group = trace.group("returned-guard").expect("returned-guard");
        assert_eq!(trace.lines_in(&group).len(), 1);
        entered
    });
    drop(returned_guard);

    capture_trace(|trace| {
        tracing::info!(target: "logs_trace::late", "after returned guard drop");
        assert_eq!(trace.lines().len(), 1);
        assert_eq!(trace.lines()[0].group, None);
    });
}

#[test]
fn entered_guard_dropped_on_another_thread_does_not_panic() {
    capture_trace(|trace| {
        let span = tracing::info_span!(target: "logs_trace::late", "cross-thread-guard");
        let guard = span.enter();
        std::thread::scope(|scope| {
            scope
                .spawn(move || {
                    drop(guard);
                })
                .join()
                .expect("dropping an entered guard on another thread must not panic");
        });
        assert!(trace.group("cross-thread-guard").is_some());
    });

    capture_trace(|trace| {
        tracing::info!(target: "logs_trace::late", "after cross-thread guard drop");
        assert_eq!(trace.lines().len(), 1);
        assert_eq!(trace.lines()[0].group, None);
    });
}

#[test]
fn span_record_adds_fields_after_group_opens() {
    // Run an earlier capture so group IDs in the next capture do not start at 1.
    capture_trace(|_| {
        let _prior = tracing::info_span!(target: "logs_trace::record", "prior-group").entered();
    });

    capture_trace(|trace| {
        let span = tracing::info_span!(
            target: "logs_trace::record",
            "recorded-group",
            status = "pending",
            outcome = tracing::field::Empty,
            count = tracing::field::Empty
        );
        let _entered = span.enter();
        span.record("status", "done");
        span.record("outcome", "saved");
        span.record("count", 4u64);

        let group = trace.group("recorded-group").expect("recorded-group");
        assert_eq!(group.fields.get("status").map(String::as_str), Some("done"));
        assert_eq!(
            group.fields.get("outcome").map(String::as_str),
            Some("saved")
        );
        assert_eq!(group.fields.get("count").map(String::as_str), Some("4"));
    });
}
