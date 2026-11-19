use gpui::TestAppContext;
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tracing::{Event, Metadata, Subscriber, span};
use yard_shell as shell;

static TEST_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Default)]
struct Saves(Arc<Mutex<Vec<&'static str>>>);

impl Saves {
    fn count(&self, name: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|&&saved| saved == name)
            .count()
    }
}

impl Subscriber for Saves {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.is_span() && matches!(metadata.name(), "store-save" | "state-save" | "file-write")
    }

    fn new_span(&self, attributes: &span::Attributes<'_>) -> span::Id {
        let mut saves = self.0.lock().unwrap();
        saves.push(attributes.metadata().name());
        span::Id::from_u64(saves.len() as u64)
    }

    fn record(&self, _: &span::Id, _: &span::Record<'_>) {}
    fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}
    fn event(&self, _: &Event<'_>) {}
    fn enter(&self, _: &span::Id) {}
    fn exit(&self, _: &span::Id) {}
}

fn with_saves(test: impl FnOnce(&Saves)) {
    let _lock = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let saves = Saves::default();
    tracing::subscriber::with_default(saves.clone(), || test(&saves));
}

fn extension(app: &TestAppContext, dir: &Path) -> shell::Extension {
    app.update(|cx| {
        shell::set_store_dir(cx, dir.into());
        shell::Extension::new(cx, "tour").unwrap()
    })
}

fn saved_value(path: &Path) -> Value {
    let contents: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    contents["values"]["count"].clone()
}

fn settle(app: &TestAppContext) {
    app.run_until_parked();
    app.executor().advance_clock(Duration::from_millis(400));
    app.run_until_parked();
}

#[gpui::test]
fn writing_during_a_save_queues_one_more_save(cx: &mut TestAppContext) {
    with_saves(|saves| {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tour.json");
        let app = cx.new_app();
        let tour = extension(&app, dir.path());
        app.update(|cx| tour.store(cx, "count", json!(1)));
        app.run_until_parked();

        let executor = app.executor();
        for _ in 0..50 {
            if saves.count("store-save") > 0 {
                break;
            }
            if !executor.tick() {
                assert!(
                    executor
                        .dispatcher()
                        .as_test()
                        .unwrap()
                        .advance_clock_to_next_timer()
                );
            }
        }
        assert_eq!(saves.count("store-save"), 1);
        assert!(!file.exists(), "the first background write has already run");
        app.update(|cx| tour.store(cx, "count", json!(2)));
        settle(&app);

        assert_eq!(saves.count("store-save"), 2);
        assert_eq!(saves.count("file-write"), 2);
        assert_eq!(saved_value(&file), json!(2));
    });
}

#[gpui::test]
fn quitting_flushes_both_waiting_files_once(cx: &mut TestAppContext) {
    with_saves(|saves| {
        let dir = tempfile::tempdir().unwrap();
        let states = dir.path().join("state.json");
        let stores = dir.path().join("store");
        let file = stores.join("tour.json");
        let app = cx.new_app();
        let tour = extension(&app, &stores);
        app.update(|cx| {
            shell::set_state_file(cx, states.clone());
            shell::state_changed(cx);
            tour.store(cx, "count", json!(3));
        });
        app.run_until_parked();
        assert!(!file.exists());
        assert!(!states.exists());

        app.quit();
        settle(&app);

        assert_eq!(saves.count("store-save"), 1);
        assert_eq!(saves.count("state-save"), 1);
        assert_eq!(saves.count("file-write"), 2);
        assert_eq!(saved_value(&file), json!(3));
        assert_eq!(
            yard_state::decode(&fs::read_to_string(states).unwrap()).unwrap(),
            vec![]
        );
    });
}

#[gpui::test]
fn storing_the_same_value_does_not_reset_or_schedule_a_save(cx: &mut TestAppContext) {
    with_saves(|saves| {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tour.json");
        let app = cx.new_app();
        let tour = extension(&app, dir.path());
        app.update(|cx| tour.store(cx, "count", json!(3)));
        advance(&app, 200);
        assert!(!file.exists());

        app.update(|cx| tour.store(cx, "count", json!(3)));
        advance(&app, 100);
        assert_eq!(saves.count("file-write"), 1);
        assert_eq!(saved_value(&file), json!(3));

        app.update(|cx| tour.store(cx, "count", json!(3)));
        settle(&app);
        assert_eq!(saves.count("file-write"), 1);
    });
}

fn advance(app: &TestAppContext, milliseconds: u64) {
    app.executor()
        .advance_clock(Duration::from_millis(milliseconds));
    app.run_until_parked();
}

#[gpui::test]
fn quitting_writes_only_stores_with_unwritten_changes(cx: &mut TestAppContext) {
    with_saves(|saves| {
        let dir = tempfile::tempdir().unwrap();
        let app = cx.new_app();
        let tour = extension(&app, dir.path());
        let guest = app.update(|cx| shell::Extension::new(cx, "guest").unwrap());

        app.update(|cx| tour.store(cx, "count", json!(1)));
        settle(&app);
        let tour_file = dir.path().join("tour.json");
        assert_eq!(saved_value(&tour_file), json!(1));
        let external_values = json!({"version": 1, "values": {"count": 9}});
        fs::write(&tour_file, external_values.to_string()).unwrap();

        app.update(|cx| guest.store(cx, "count", json!(2)));
        app.quit();
        app.run_until_parked();
        assert_eq!(saves.count("store-save"), 2);
        assert_eq!(saves.count("file-write"), 2);
        assert_eq!(saved_value(&dir.path().join("guest.json")), json!(2));
        assert_eq!(saved_value(&tour_file), json!(9));
    });
}

#[gpui::test]
fn quitting_retries_a_failed_background_store_write(cx: &mut TestAppContext) {
    with_saves(|saves| {
        let dir = tempfile::tempdir().unwrap();
        let stores = dir.path().join("store");
        let file = stores.join("tour.json");
        let app = cx.new_app();
        let tour = extension(&app, &stores);
        app.update(|cx| tour.store(cx, "count", json!(1)));

        fs::write(&stores, "this file blocks creation of the store directory").unwrap();
        settle(&app);
        assert_eq!(saves.count("file-write"), 1);
        assert!(!file.exists());
        fs::remove_file(&stores).unwrap();

        app.quit();
        app.run_until_parked();
        assert_eq!(saves.count("file-write"), 2);
        assert_eq!(saved_value(&file), json!(1));
    });
}

#[gpui::test]
fn a_second_store_folder_is_refused(cx: &mut TestAppContext) {
    with_saves(|_| {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let app = cx.new_app();
        let tour = extension(&app, first.path());
        app.update(|cx| {
            shell::set_store_dir(cx, second.path().into());
            tour.store(cx, "count", json!(1));
        });
        settle(&app);
        assert_eq!(saved_value(&first.path().join("tour.json")), json!(1));
        assert!(!second.path().join("tour.json").exists());
    });
}

#[gpui::test]
fn setting_a_store_folder_after_first_access_is_refused(cx: &mut TestAppContext) {
    with_saves(|_| {
        let dir = tempfile::tempdir().unwrap();
        let app = cx.new_app();
        let tour = app.update(|cx| shell::Extension::new(cx, "tour").unwrap());
        app.update(|cx| {
            assert_eq!(tour.stored(cx, "count"), None);
            shell::set_store_dir(cx, dir.path().into());
            tour.store(cx, "count", json!(2));
            assert_eq!(tour.stored(cx, "count"), Some(json!(2)));
        });
        settle(&app);
        app.quit();
        app.run_until_parked();
        assert!(!dir.path().join("tour.json").exists());
    });
}

#[gpui::test]
fn continuous_writes_save_at_two_second_intervals_until_the_trailing_save(cx: &mut TestAppContext) {
    with_saves(|saves| {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tour.json");
        let app = cx.new_app();
        let tour = extension(&app, dir.path());

        for tick in 0..50 {
            app.update(|cx| tour.store(cx, "count", json!(tick)));
            app.run_until_parked();
            advance(&app, 100);
            let writes = (tick + 1) / 20;
            assert_eq!(saves.count("file-write"), writes, "tick {tick}");
            if writes > 0 {
                assert_eq!(saved_value(&file), json!(writes * 20 - 1));
            }
        }

        advance(&app, 199);
        assert_eq!(saves.count("file-write"), 2);
        advance(&app, 1);
        assert_eq!(saves.count("file-write"), 3);
        assert_eq!(saved_value(&file), json!(49));
    });
}

#[gpui::test]
fn each_store_and_the_state_file_keep_their_own_wait(cx: &mut TestAppContext) {
    with_saves(|saves| {
        let dir = tempfile::tempdir().unwrap();
        let states = dir.path().join("state.json");
        let tour_file = dir.path().join("tour.json");
        let guest_file = dir.path().join("guest.json");
        let app = cx.new_app();
        let tour = extension(&app, dir.path());
        let guest = app.update(|cx| {
            shell::set_state_file(cx, states.clone());
            shell::Extension::new(cx, "guest").unwrap()
        });

        app.update(|cx| tour.store(cx, "count", json!(1)));
        advance(&app, 100);
        app.update(|cx| guest.store(cx, "count", json!(2)));
        advance(&app, 100);
        app.update(shell::state_changed);
        advance(&app, 99);
        assert_eq!(saves.count("file-write"), 0);

        advance(&app, 1);
        assert_eq!(saved_value(&tour_file), json!(1));
        assert!(!guest_file.exists());
        assert!(!states.exists());
        app.update(|cx| tour.store(cx, "count", json!(3)));

        advance(&app, 100);
        assert_eq!(saved_value(&guest_file), json!(2));
        assert!(!states.exists());
        assert_eq!(saved_value(&tour_file), json!(1));
        app.update(|cx| guest.store(cx, "count", json!(4)));

        advance(&app, 100);
        assert!(states.exists());
        assert_eq!(saves.count("file-write"), 3);
        advance(&app, 100);
        assert_eq!(saved_value(&tour_file), json!(3));
        assert_eq!(saved_value(&guest_file), json!(2));
        advance(&app, 100);
        assert_eq!(saved_value(&guest_file), json!(4));
        assert_eq!(saves.count("file-write"), 5);
    });
}

#[gpui::test]
fn path_like_keys_stay_in_the_extensions_own_file(cx: &mut TestAppContext) {
    with_saves(|_| {
        let dir = tempfile::tempdir().unwrap();
        let stores = dir.path().join("store");
        let app = cx.new_app();
        let tour = extension(&app, &stores);
        app.update(|cx| {
            tour.store(cx, "../x", json!(1));
            tour.store(cx, "a/b", json!(2));
            assert_eq!(tour.stored(cx, "../x"), Some(json!(1)));
            assert_eq!(tour.stored(cx, "a/b"), Some(json!(2)));
        });
        settle(&app);
        let contents: Value =
            serde_json::from_slice(&fs::read(stores.join("tour.json")).unwrap()).unwrap();
        assert_eq!(contents["values"], json!({"../x": 1, "a/b": 2}));
        assert_eq!(fs::read_dir(stores).unwrap().count(), 1);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    });
}

#[gpui::test]
fn the_store_is_read_only_on_first_access(cx: &mut TestAppContext) {
    with_saves(|_| {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tour.json");
        let write = |count| {
            fs::write(
                &file,
                json!({"version": 1, "values": {"count": count}}).to_string(),
            )
            .unwrap();
        };
        write(1);
        let app = cx.new_app();
        let tour = extension(&app, dir.path());

        assert_eq!(app.update(|cx| tour.stored(cx, "count")), Some(json!(1)));
        write(2);
        assert_eq!(app.update(|cx| tour.stored(cx, "count")), Some(json!(1)));
        app.update(|cx| tour.store(cx, "other", json!(true)));
        settle(&app);
        assert_eq!(saved_value(&file), json!(1));
    });
}
