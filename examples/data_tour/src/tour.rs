use gpui::{
    App, AppContext, AsyncApp, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window,
};
use tracing::Instrument;
use yard_shell::{DataError, Db, Extension, rusqlite};
use yard_ui::{Button, Label, palette, v_flex};

const MIGRATIONS: &[&str] = &[
    r#"CREATE TABLE "data_tour.notes" (id INTEGER PRIMARY KEY, text TEXT NOT NULL)"#,
    r#"ALTER TABLE "data_tour.notes" ADD COLUMN done INTEGER NOT NULL DEFAULT 0"#,
];
const PEER_MIGRATIONS: &[&str] = &[r#"
    CREATE TABLE "data_peer.secrets" (text TEXT NOT NULL);
    INSERT INTO "data_peer.secrets" (text) VALUES ('Peer row');
"#];

#[derive(Clone, Copy)]
enum Mode {
    On,
    Old,
    Peek,
}

fn mode() -> Option<Mode> {
    match std::env::var("DATA_TOUR").as_deref() {
        Ok("on") => Some(Mode::On),
        Ok("old") => Some(Mode::Old),
        Ok("peek") => Some(Mode::Peek),
        _ => None,
    }
}

pub fn peer(cx: &mut App, ext: &Extension) {
    if mode().is_none() {
        return;
    }
    let _group = tracing::info_span!("data-peer-init").entered();
    ext.migrate_data(cx, PEER_MIGRATIONS);
    tracing::debug!("queued peer migration");
}

pub fn init(cx: &mut App, ext: &Extension) {
    let Some(mode) = mode() else {
        return;
    };
    let _group = tracing::info_span!("data-tour-init").entered();
    let migrations = if matches!(mode, Mode::Old) {
        &MIGRATIONS[..1]
    } else {
        MIGRATIONS
    };
    ext.migrate_data(cx, migrations);
    tracing::debug!(steps = migrations.len(), "queued tour migrations");

    let tour = ext.clone();
    ext.register(cx, "data_tour.card", &[], move |_, _, _, cx| {
        cx.new(|cx| {
            let mut card = Card {
                extension: tour.clone(),
                notes: Vec::new(),
                status: String::from("loading"),
                pending: false,
                can_peek: matches!(mode, Mode::Peek),
            };
            card.request(Action::Load, cx);
            card
        })
        .into()
    });
    ext.place_root(cx, "data-tour-card", "data_tour.card");
    tracing::debug!("placed the data tour card");
}

struct Note {
    text: String,
    done: bool,
}

#[derive(Clone, Copy)]
enum Action {
    Load,
    Add,
    Done,
    Peek,
}

impl Action {
    fn span(self) -> tracing::Span {
        match self {
            Self::Load => tracing::info_span!("data-tour-load"),
            Self::Add => tracing::info_span!("data-tour-add"),
            Self::Done => tracing::info_span!("data-tour-done"),
            Self::Peek => tracing::info_span!("data-tour-peek"),
        }
    }

    async fn run(self, ext: &Extension, cx: &AsyncApp) -> Result<Option<Vec<Note>>, DataError> {
        match self {
            Self::Load => {}
            Self::Add => {
                tracing::debug!("requesting a note insert");
                cx.update(|cx| {
                    ext.write_data(cx, |db: &Db| {
                        db.execute(
                            r#"INSERT INTO "data_tour.notes" (text)
                               SELECT 'Note ' || (COUNT(*) + 1) FROM "data_tour.notes""#,
                            [],
                        )
                    })
                })
                .await?;
            }
            Self::Done => {
                tracing::debug!("requesting an update to the oldest unfinished note");
                let updated = cx
                    .update(|cx| {
                        ext.write_data(cx, |db: &Db| {
                            db.execute(
                                r#"UPDATE "data_tour.notes" SET done = 1
                                   WHERE id = (
                                       SELECT id FROM "data_tour.notes" WHERE done = 0
                                       ORDER BY id LIMIT 1
                                   )"#,
                                [],
                            )
                        })
                    })
                    .await?;
                if updated == 0 {
                    tracing::info!("no unfinished notes");
                }
            }
            Self::Peek => {
                tracing::debug!("requesting a read from the peer");
                cx.update(|cx| {
                    ext.read_data(cx, |db: &Db| {
                        db.query_row(r#"SELECT text FROM "data_peer.secrets""#, [], |row| {
                            row.get::<_, String>(0)
                        })
                    })
                })
                .await?;
                return Ok(None);
            }
        }

        tracing::debug!("requesting the note list");
        let notes = cx
            .update(|cx| {
                ext.read_data(cx, |db: &Db| {
                    let mut query =
                        db.prepare(r#"SELECT text, done FROM "data_tour.notes" ORDER BY id"#)?;
                    query
                        .query_map([], |row| {
                            Ok(Note {
                                text: row.get(0)?,
                                done: row.get(1)?,
                            })
                        })?
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
            })
            .await?;
        Ok(Some(notes))
    }
}

struct Card {
    extension: Extension,
    notes: Vec<Note>,
    status: String,
    pending: bool,
    can_peek: bool,
}

impl Card {
    fn request(&mut self, action: Action, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        self.pending = true;
        cx.notify();

        let span = action.span();
        let ext = self.extension.clone();
        cx.spawn(async move |view, cx| {
            async {
                let result = action.run(&ext, cx).await;
                tracing::debug!(ok = result.is_ok(), "request finished");
                let _ = view.update(cx, |card, cx| {
                    card.pending = false;
                    card.status = match result {
                        Ok(notes) => {
                            if let Some(notes) = notes {
                                card.notes = notes;
                            }
                            "ok".into()
                        }
                        Err(error) => error_status(error),
                    };
                    cx.notify();
                });
            }
            .instrument(span)
            .await;
        })
        .detach();
    }
}

fn error_status(error: DataError) -> String {
    match error {
        DataError::Unavailable => "unavailable".into(),
        DataError::Newer { .. } => "data is from a newer version".into(),
        DataError::Busy => "busy".into(),
        DataError::Sql(ref sql)
            if sql.sqlite_error_code()
                == Some(rusqlite::ErrorCode::AuthorizationForStatementDenied) =>
        {
            "refused".into()
        }
        error => error.to_string(),
    }
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!("rendering the data tour card");
        let colors = palette(cx);
        let button = |id, label, action| {
            let view = cx.weak_entity();
            Button::new(id, label)
                .disabled(self.pending)
                .on_click(move |_, cx| {
                    let _ = view.update(cx, |card, cx| card.request(action, cx));
                })
        };
        v_flex()
            .size_full()
            .p_2()
            .gap_2()
            .border_1()
            .border_color(colors.border)
            .child(Label::new("Data tour"))
            .child(Label::new(self.status.clone()))
            .child(button("data-tour-add", "Add note", Action::Add))
            .child(button("data-tour-done", "Mark done", Action::Done))
            .children(
                self.can_peek
                    .then(|| button("data-tour-peek", "Peek", Action::Peek)),
            )
            .child(
                v_flex()
                    .id("data-tour-notes")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(self.notes.iter().map(|note| {
                        let done = if note.done { "done" } else { "not done" };
                        Label::new(format!("{} ({done})", note.text))
                    })),
            )
    }
}
