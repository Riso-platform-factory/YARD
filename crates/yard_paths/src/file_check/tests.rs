use super::*;

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-paths-file-check-{}-{}-{nanos:x}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_first_check_reports_the_state_of_the_path() {
    let temp = TempDir::new();

    let missing = temp.path().join("missing.json");
    let mut check = FileCheck::new(missing.clone());
    assert_eq!(check.check(), Some(FileState::Missing));
    assert!(!missing.exists());

    // `new` reads nothing: a file written after creation is still the first state seen.
    let path = temp.path().join("settings.json");
    let mut check = FileCheck::new(path.clone());
    std::fs::write(&path, br#"{"a": 1}"#).unwrap();
    assert_eq!(
        check.check(),
        Some(FileState::Bytes(br#"{"a": 1}"#.to_vec()))
    );
}

#[test]
fn an_unchanged_path_reports_nothing_after_the_first_check() {
    let temp = TempDir::new();

    let missing = temp.path().join("missing.json");
    let mut check = FileCheck::new(missing);
    assert_eq!(check.check(), Some(FileState::Missing));
    assert_eq!(check.check(), None);
    assert_eq!(check.check(), None);

    let path = temp.path().join("settings.json");
    std::fs::write(&path, br#"{"a": 1}"#).unwrap();
    let mut check = FileCheck::new(path);
    assert_eq!(
        check.check(),
        Some(FileState::Bytes(br#"{"a": 1}"#.to_vec()))
    );
    assert_eq!(check.check(), None);
    assert_eq!(check.check(), None);
}

#[test]
fn a_change_of_the_same_length_reports_the_new_bytes() {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    std::fs::write(&path, br#"{"a": 1}"#).unwrap();
    let mut check = FileCheck::new(path.clone());
    assert_eq!(
        check.check(),
        Some(FileState::Bytes(br#"{"a": 1}"#.to_vec()))
    );

    std::fs::write(&path, br#"{"b": 2}"#).unwrap();
    assert_eq!(
        check.check(),
        Some(FileState::Bytes(br#"{"b": 2}"#.to_vec()))
    );
    assert_eq!(check.check(), None);
}

#[test]
fn a_file_that_appears_then_disappears_reports_each_change() {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    let mut check = FileCheck::new(path.clone());
    assert_eq!(check.check(), Some(FileState::Missing));
    assert_eq!(check.check(), None);

    std::fs::write(&path, br#"{"a": 1}"#).unwrap();
    assert_eq!(
        check.check(),
        Some(FileState::Bytes(br#"{"a": 1}"#.to_vec()))
    );
    assert_eq!(check.check(), None);

    std::fs::remove_file(&path).unwrap();
    assert_eq!(check.check(), Some(FileState::Missing));
    assert_eq!(check.check(), None);
}

#[test]
fn an_unreadable_path_is_reported_once_while_its_reason_stays() {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    std::fs::create_dir(&path).unwrap();
    let mut check = FileCheck::new(path.clone());
    let Some(FileState::Unreadable(reason)) = check.check() else {
        panic!("a directory must report FileState::Unreadable");
    };
    assert!(!reason.trim().is_empty());
    assert_eq!(check.check(), None);
    assert_eq!(check.check(), None);

    // A different state after the unreadable one is a change again.
    std::fs::remove_dir(&path).unwrap();
    std::fs::write(&path, br#"{"b": 2}"#).unwrap();
    assert_eq!(
        check.check(),
        Some(FileState::Bytes(br#"{"b": 2}"#.to_vec()))
    );
    assert_eq!(check.check(), None);
}
