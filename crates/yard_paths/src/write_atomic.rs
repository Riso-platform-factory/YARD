//! Writing a file in one step: a reader sees the old content or the new
//! content, never a partial file.

use std::{
    fs,
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// Replaces `path` with `bytes` in one step: writes a temporary file beside
/// it, syncs it, and renames it over `path`. Creates the parent directories.
/// A reader sees the old content or the new content, never a partial file.
/// A write that fails is a warning in the diagnostic log, with the path and
/// the error; the caller decides what the failure means.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let _span = tracing::info_span!("file-write", path = %path.display()).entered();
    let result = write_in_one_step(
        path,
        bytes,
        || Ok(true),
        |path, error| {
            log::warn!(
                "could not remove the temporary file {}: {error}",
                path.display()
            )
        },
    )
    .map(|_| ());
    match &result {
        Ok(()) => tracing::debug!(bytes = bytes.len(), "written"),
        Err(error) => log::warn!("could not write {}: {error}", path.display()),
    }
    result
}

/// Replaces a file only if the check succeeds immediately before its rename.
/// Returns whether the file was replaced. The caller logs any returned error.
pub fn write_atomic_if(
    path: &Path,
    bytes: &[u8],
    check: impl FnOnce() -> io::Result<bool>,
) -> io::Result<bool> {
    let _span = tracing::info_span!("file-write", path = %path.display()).entered();
    let result = write_in_one_step(path, bytes, check, |path, error| {
        tracing::warn!(path = %path.display(), %error, "could not remove the temporary file");
    });
    if let Ok(replaced) = result {
        if replaced {
            tracing::debug!(bytes = bytes.len(), "written");
        } else {
            tracing::debug!("replacement skipped");
        }
    }
    result
}

fn write_in_one_step(
    path: &Path,
    bytes: &[u8],
    check: impl FnOnce() -> io::Result<bool>,
    cleanup_failed: impl FnOnce(&Path, &io::Error),
) -> io::Result<bool> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    // The time keeps the name apart from a file that a crashed process with
    // the same ID left behind.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let temporary = parent.join(format!(
        ".{}.{}.{now:x}.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    // When this fails, there is no file of this call to remove.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    // The handle is closed before the rename, so no handle of this write is
    // open when the file moves: on Windows an open handle can make the move
    // fail. `fs::rename` replaces an existing `path` on every platform, on
    // Windows with `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`; it fails when
    // another process holds `path` open.
    drop(file);
    match written.and_then(|()| {
        if check()? {
            fs::rename(&temporary, path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }) {
        Ok(true) => {
            // So that the new name survives a power failure. Not every Unix
            // can sync a directory, and the content is safe anyway. Windows
            // cannot open a folder as a file, and NTFS journals the rename.
            #[cfg(unix)]
            if let Err(error) = fs::File::open(parent).and_then(|directory| directory.sync_all()) {
                tracing::debug!(%error, "folder not synced");
            }
            Ok(true)
        }
        result => {
            if let Err(cleanup) = fs::remove_file(&temporary)
                && cleanup.kind() != io::ErrorKind::NotFound
            {
                cleanup_failed(&temporary, &cleanup);
            }
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_replacement_keeps_the_intervening_edit_and_cleans_up() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("settings.json");
        fs::write(&target, b"original").unwrap();

        let replaced = write_atomic_if(&target, b"yard edit", || {
            fs::write(&target, b"editor edit")?;
            Ok(false)
        })
        .unwrap();

        assert!(!replaced);
        assert_eq!(fs::read(&target).unwrap(), b"editor edit");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn write_atomic_creates_parents_replaces_content_and_leaves_no_temporary_file() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a/b/file.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        let names: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["file.json"]);
    }

    #[test]
    fn a_write_that_fails_leaves_no_temporary_file() {
        let root = tempfile::tempdir().unwrap();
        // The target is a folder, so the rename fails.
        let target = root.path().join("taken");
        fs::create_dir_all(&target).unwrap();
        assert!(write_atomic(&target, b"x").is_err());
        let names: Vec<_> = fs::read_dir(root.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["taken"]);
    }
}
