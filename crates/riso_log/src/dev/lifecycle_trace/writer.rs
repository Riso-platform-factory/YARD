use super::{Outcome, Record};
use serde_json::{Map, Value};
use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufWriter, Write},
    sync::mpsc,
};

pub(crate) enum Message {
    Record(Record),
    Flush(mpsc::SyncSender<()>),
    /// The file of the lifecycle log, once the application's directories are known, or
    /// none when it could not be created.
    File(Option<File>),
}

pub(crate) fn run(rx: mpsc::Receiver<Message>, color: bool, name: &str) {
    let mut file: Option<BufWriter<File>> = None;
    // Records that wait for the file. `None` once the file is decided.
    let mut waiting: Option<Vec<Record>> = Some(Vec::new());
    let mut groups = HashMap::<u64, (String, u64)>::new();
    let mut reported = false;
    let mut report = |result: io::Result<()>| {
        if let Err(error) = result
            && !reported
        {
            let _ = writeln!(io::stderr(), "{name} lifecycle write failed: {error}");
            reported = true;
        }
    };
    while let Ok(mut message) = rx.recv() {
        let mut stderr = io::stderr().lock();
        loop {
            match message {
                Message::Record(record) => {
                    report(writeln!(stderr, "{}", console(&record, &mut groups, color)));
                    match (&mut file, &mut waiting) {
                        (Some(file), _) => report(write_record(file, &record)),
                        (None, Some(waiting)) => waiting.push(record),
                        (None, None) => {}
                    }
                }
                Message::File(opened) => {
                    file = opened.map(BufWriter::new);
                    for record in waiting.take().unwrap_or_default() {
                        if let Some(file) = &mut file {
                            report(write_record(file, &record));
                        }
                    }
                }
                Message::Flush(ack) => {
                    if let Some(file) = &mut file {
                        report(file.flush());
                    }
                    report(stderr.flush());
                    let _ = ack.send(());
                }
            }
            match rx.try_recv() {
                Ok(next) => message = next,
                Err(_) => break,
            }
        }
        if let Some(file) = &mut file {
            report(file.flush());
        }
        report(stderr.flush());
    }
}

/// A message as the console shows it: its later lines are indented under the
/// first, and other control characters are shown as escapes, so a message
/// cannot pass for another record.
fn readable(message: &str) -> String {
    let mut text = String::with_capacity(message.len());
    for c in message.chars() {
        match c {
            '\n' => text.push_str("\n           "),
            '\t' => text.push(c),
            c if c.is_control() => text.extend(c.escape_default()),
            c => text.push(c),
        }
    }
    text
}

fn write_record(file: &mut BufWriter<File>, record: &Record) -> io::Result<()> {
    serde_json::to_writer(&mut *file, record)
        .map_err(io::Error::other)
        .and_then(|()| file.write_all(b"\n"))
}

fn fields_text(fields: &Map<String, Value>) -> String {
    if fields.is_empty() {
        return String::new();
    }
    let fields = fields
        .iter()
        .map(|(key, value)| {
            format!(
                "{key}={}",
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string())
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{fields}}}")
}

fn console(record: &Record, groups: &mut HashMap<u64, (String, u64)>, color: bool) -> String {
    let (t, text) = match record {
        Record::RunStart {
            t,
            name,
            version,
            os,
        } => (*t, format!("run-start {name} {version} ({os})")),
        Record::RunEnd { t } => (*t, "run-end".into()),
        Record::GroupStart {
            t,
            id,
            parent,
            name,
            fields,
            ..
        } => {
            let prefix = parent
                .map(|id| format!("{} > ", path(groups, id)))
                .unwrap_or_default();
            let path = format!("{prefix}{name}{}", fields_text(fields));
            let text = format!("▶ [{path}]");
            groups.insert(*id, (path, *t));
            (*t, text)
        }
        Record::Line {
            t,
            group,
            level,
            message,
            known,
            fields,
            ..
        } => {
            let fields = fields_text(fields);
            let suffix = if fields.is_empty() {
                fields
            } else {
                format!(" {fields}")
            };
            let known = if level == "ERROR" {
                known
                    .as_ref()
                    .map(|slug| format!(" [known: {slug}]"))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            (
                *t,
                format!(
                    "{level:<5} [{}] {}{suffix}{known}",
                    path(groups, *group),
                    readable(message)
                ),
            )
        }
        Record::GroupEnd { t, id, outcome } => {
            let (path, start) = groups.remove(id).unwrap_or_else(|| ("?".into(), *t));
            let elapsed = t.saturating_sub(start);
            let duration = if elapsed >= 1_000_000 {
                format!("{:.2}s", elapsed as f64 / 1_000_000.0)
            } else if elapsed < 500 {
                "<1ms".into()
            } else {
                format!("{:.0}ms", elapsed as f64 / 1_000.0)
            };
            let symbol = match outcome {
                Outcome::Success => "✓",
                Outcome::KnownFailure => "⚠",
                Outcome::Failure => "✗",
                Outcome::Interrupted => "⊘",
            };
            (*t, format!("{symbol} [{path}] ({duration})"))
        }
    };
    let line = format!("{:8.3}s {text}", t as f64 / 1_000_000.0);
    let ansi = if color {
        match record {
            Record::Line { level, known, .. } if level == "ERROR" => Some(if known.is_some() {
                "\x1b[38;5;208m"
            } else {
                "\x1b[31m"
            }),
            Record::GroupEnd {
                outcome: Outcome::Failure,
                ..
            } => Some("\x1b[31m"),
            Record::GroupEnd {
                outcome: Outcome::KnownFailure,
                ..
            } => Some("\x1b[38;5;208m"),
            _ => None,
        }
    } else {
        None
    };
    match ansi {
        Some(ansi) => format!("{ansi}{line}\x1b[0m"),
        None => line,
    }
}

fn path(groups: &HashMap<u64, (String, u64)>, id: u64) -> &str {
    groups
        .get(&id)
        .map(|(path, _)| path.as_str())
        .unwrap_or("?")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_written_before_the_file_opens_are_written_to_it() {
        let path = std::env::temp_dir().join(format!("sample-writer-{}.jsonl", std::process::id()));
        let line = |message: &str| Record::Line {
            t: 1,
            group: 1,
            level: "INFO".into(),
            target: "test".into(),
            message: message.into(),
            fields: Map::new(),
            known: None,
        };
        let (tx, rx) = mpsc::channel();
        let writer = std::thread::spawn(move || run(rx, false, "sample"));
        tx.send(Message::Record(line("before"))).unwrap();
        tx.send(Message::File(Some(File::create(&path).unwrap())))
            .unwrap();
        tx.send(Message::Record(line("after"))).unwrap();
        drop(tx);
        writer.join().unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        let messages: Vec<_> = written
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap()["message"].clone())
            .collect();
        assert_eq!(messages, ["before", "after"]);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn errors_and_failed_groups_have_exact_terminal_colors() {
        let unknown = Record::Line {
            t: 1_000_000,
            group: 1,
            level: "ERROR".into(),
            target: "test".into(),
            message: "problem".into(),
            fields: Map::new(),
            known: None,
        };
        let known = Record::Line {
            t: 1_000_000,
            group: 1,
            level: "ERROR".into(),
            target: "test".into(),
            message: "problem".into(),
            fields: Map::new(),
            known: Some("test-known".into()),
        };
        let known_end = Record::GroupEnd {
            t: 1_000_000,
            id: 1,
            outcome: Outcome::KnownFailure,
        };
        let failed_end = Record::GroupEnd {
            t: 1_000_000,
            id: 1,
            outcome: Outcome::Failure,
        };
        let success_end = Record::GroupEnd {
            t: 1_000_000,
            id: 1,
            outcome: Outcome::Success,
        };
        for (record, plain, ansi) in [
            (unknown, "   1.000s ERROR [work] problem", "\x1b[31m"),
            (
                known,
                "   1.000s ERROR [work] problem [known: test-known]",
                "\x1b[38;5;208m",
            ),
            (known_end, "   1.000s ⚠ [work] (1.00s)", "\x1b[38;5;208m"),
            (failed_end, "   1.000s ✗ [work] (1.00s)", "\x1b[31m"),
            (success_end, "   1.000s ✓ [work] (1.00s)", ""),
        ] {
            for color in [true, false] {
                let mut groups = HashMap::from([(1, ("work".into(), 0))]);
                let actual = console(&record, &mut groups, color);
                let expected = if color && !ansi.is_empty() {
                    format!("{ansi}{plain}\x1b[0m")
                } else {
                    plain.into()
                };
                assert_eq!(actual, expected);
                if !color {
                    assert!(!actual.contains('\x1b'));
                }
            }
            let json = serde_json::to_string(&record).unwrap();
            assert!(!json.contains('\x1b'));
            assert!(!json.contains("\\u001b"));
        }
    }

    #[test]
    fn the_console_shows_group_paths_durations_and_one_line_per_message() {
        let mut groups = HashMap::new();
        let start = |id, parent, name: &str, fields: Map<String, Value>| Record::GroupStart {
            t: 0,
            id,
            parent,
            name: name.into(),
            fields,
            thread: "main".into(),
        };
        let mut fields = Map::new();
        fields.insert("extension".into(), "agents".into());
        assert_eq!(
            console(
                &start(1, None, "app-startup", Map::new()),
                &mut groups,
                false
            ),
            "   0.000s ▶ [app-startup]"
        );
        assert_eq!(
            console(
                &start(2, Some(1), "extension-init", fields),
                &mut groups,
                false
            ),
            "   0.000s ▶ [app-startup > extension-init{extension=agents}]"
        );
        let line = Record::Line {
            t: 300,
            group: 2,
            level: "DEBUG".into(),
            target: "test".into(),
            message: "two\nlines\u{1b}[31m".into(),
            fields: Map::new(),
            known: None,
        };
        assert_eq!(
            console(&line, &mut groups, false),
            "   0.000s DEBUG [app-startup > extension-init{extension=agents}] two\n           lines\\u{1b}[31m"
        );
        let end = |id, t, outcome| Record::GroupEnd { t, id, outcome };
        assert_eq!(
            console(&end(2, 400, Outcome::Success), &mut groups, false),
            "   0.000s ✓ [app-startup > extension-init{extension=agents}] (<1ms)"
        );
        assert_eq!(
            console(&end(1, 3_000, Outcome::Interrupted), &mut groups, false),
            "   0.003s ⊘ [app-startup] (3ms)"
        );
    }
}
