use super::{check_open, check_reveal, check_url};
use std::fs;
use std::path::{Path, PathBuf};

fn expected_canonical(path: &Path) -> PathBuf {
    let canonical = fs::canonicalize(path).unwrap();
    #[cfg(windows)]
    {
        if let Some(stripped) = canonical.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
            return PathBuf::from(stripped);
        }
    }
    canonical
}

fn write_plain_text(path: &Path) {
    fs::write(path, "hello").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
    }
}

#[test]
fn check_reveal_accepts_text_file_folder_and_executable() {
    let dir = tempfile::tempdir().unwrap();

    let text = dir.path().join("notes.txt");
    write_plain_text(&text);
    assert_eq!(check_reveal(&text), Ok(text.clone()));

    let folder = dir.path().join("folder");
    fs::create_dir(&folder).unwrap();
    assert_eq!(check_reveal(&folder), Ok(folder.clone()));

    let executable = dir.path().join("probe.sh");
    fs::write(&executable, "#!/bin/sh\necho hi\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert_eq!(check_reveal(&executable), Ok(executable.clone()));
}

#[test]
fn check_open_accepts_text_file_and_returns_canonical_path() {
    let dir = tempfile::tempdir().unwrap();
    let text = dir.path().join("notes.txt");
    write_plain_text(&text);

    assert_eq!(check_open(&text), Ok(expected_canonical(&text)));
}

#[test]
fn check_open_accepts_pdf_and_markdown_files() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["x.pdf", "notes.md"] {
        let file = dir.path().join(name);
        write_plain_text(&file);
        assert_eq!(
            check_open(&file),
            Ok(expected_canonical(&file)),
            "check_open must accept plain non-executable `{name}`"
        );
    }
}

#[cfg(unix)]
#[test]
fn check_open_accepts_symlink_to_text_file_and_returns_target_canonical_path() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.txt");
    write_plain_text(&target);

    let link = dir.path().join("notes.txt");
    symlink(&target, &link).unwrap();

    assert_eq!(check_open(&link), Ok(expected_canonical(&target)));
}

#[cfg(windows)]
#[test]
fn check_reveal_normalizes_forward_slashes_to_backslashes() {
    let dir = tempfile::tempdir().unwrap();
    let text = dir.path().join("notes.txt");
    write_plain_text(&text);

    let expected = text.to_str().unwrap().replace('/', "\\");
    let with_slashes = expected.replace('\\', "/");
    assert!(with_slashes.contains('/'));

    assert_eq!(
        check_reveal(Path::new(&with_slashes)),
        Ok(PathBuf::from(&expected))
    );
}

#[test]
fn check_url_returns_parsed_url_for_each_accepted_url() {
    let cases = [
        ("https://example.com/", "https://example.com/"),
        ("HTTPS://Example.COM/a?b=c#d", "https://example.com/a?b=c#d"),
        ("http://127.0.0.1:8080/x", "http://127.0.0.1:8080/x"),
        (
            "mailto:someone@example.com?subject=Hi",
            "mailto:someone@example.com?subject=Hi",
        ),
        (
            "mailto:a@example.com,b@example.com",
            "mailto:a@example.com,b@example.com",
        ),
        (
            "mailto:a@example.com?body=line1%0D%0Aline2",
            "mailto:a@example.com?body=line1%0D%0Aline2",
        ),
        (
            "mailto:a@example.com?BODY=line1%0D%0Aline2",
            "mailto:a@example.com?BODY=line1%0D%0Aline2",
        ),
        (
            "mailto:a@example.com?cc=b@example.com&bcc=c@example.com",
            "mailto:a@example.com?cc=b@example.com&bcc=c@example.com",
        ),
        ("https://[::1]:8080/", "https://[::1]:8080/"),
        ("https://bücher.example/", "https://xn--bcher-kva.example/"),
        ("HtTpS://Example.COM/Path", "https://example.com/Path"),
    ];

    for (input, expected) in cases {
        let parsed = check_url(input).unwrap_or_else(|error| {
            panic!("expected check_url({input:?}) to succeed, got {error:?}")
        });
        assert_eq!(parsed, url::Url::parse(expected).unwrap());
        assert_eq!(parsed.as_str(), expected);
    }
}
