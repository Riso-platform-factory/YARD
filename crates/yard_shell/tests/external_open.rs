#![cfg(feature = "dev")]

use gpui::TestAppContext;
use std::fs;
use std::path::{Path, PathBuf};
use yard_shell::{Extension, ExternalOpenError, MAX_URL_LEN};

fn extension(cx: &mut TestAppContext) -> Extension {
    cx.update(|cx| Extension::new(cx, "tour").unwrap())
}

fn expected_message(error: ExternalOpenError) -> &'static str {
    match error {
        ExternalOpenError::InvalidUrl => "invalid URL",
        ExternalOpenError::UrlTooLong => "URL too long",
        ExternalOpenError::SchemeNotAllowed => "scheme not allowed",
        ExternalOpenError::InvalidPath => "invalid path",
        ExternalOpenError::NotFound => "path not found",
        ExternalOpenError::NotAFile => "not a file",
        ExternalOpenError::Executable => "executable file refused",
    }
}

fn assert_error_hides_input(error: ExternalOpenError, input: &str, needles: &[&str]) {
    let text = error.to_string();
    assert_eq!(
        text,
        expected_message(error),
        "unexpected Display text for {error:?}"
    );
    if !input.is_empty() {
        assert!(
            !text.contains(input),
            "error text `{text}` must not contain input `{input}`"
        );
    }
    for needle in needles {
        assert!(
            !text.contains(needle),
            "error text `{text}` must not contain `{needle}`"
        );
    }
}

#[gpui::test]
fn accepted_urls_reach_gpui_as_serialized_by_url_crate(cx: &mut TestAppContext) {
    let ext = extension(cx);
    assert_eq!(cx.opened_url(), None);

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
        let result = cx.read(|cx| ext.open_url(input, cx));
        assert_eq!(result, Ok(()), "expected `{input}` to be accepted");
        assert_eq!(
            cx.opened_url().as_deref(),
            Some(expected),
            "expected `{input}` to reach GPUI serialized as `{expected}`"
        );
    }
}

#[gpui::test]
fn url_at_exact_max_len_is_accepted_and_one_byte_over_is_refused(cx: &mut TestAppContext) {
    let ext = extension(cx);
    let prefix = "https://example.com/";
    let exact_max = format!("{prefix}{}", "a".repeat(MAX_URL_LEN - prefix.len()));
    let over_max = format!("{exact_max}a");
    assert_eq!(exact_max.len(), MAX_URL_LEN);
    assert_eq!(over_max.len(), MAX_URL_LEN + 1);

    let refused = cx.read(|cx| ext.open_url(&over_max, cx));
    assert_eq!(refused, Err(ExternalOpenError::UrlTooLong));
    assert_error_hides_input(ExternalOpenError::UrlTooLong, &over_max, &["example.com"]);
    assert_eq!(cx.opened_url(), None, "over-length URL must not reach GPUI");

    let accepted = cx.read(|cx| ext.open_url(&exact_max, cx));
    assert_eq!(
        accepted,
        Ok(()),
        "URL of exactly MAX_URL_LEN bytes must not be refused for its length"
    );
    assert_eq!(cx.opened_url().as_deref(), Some(exact_max.as_str()));
}

#[gpui::test]
fn url_over_max_len_after_serialization_is_refused(cx: &mut TestAppContext) {
    let ext = extension(cx);
    assert_eq!(cx.opened_url(), None);

    let prefix = "https://example.com/";
    // Each `é` is 2 UTF-8 bytes raw, but percent-encodes to `%C3%A9` (6 bytes).
    // 20 + 1363 * 2 = 2746 <= 8192 raw, 20 + 1363 * 6 = 8198 > 8192 serialized.
    let raw = format!("{prefix}{}", "é".repeat(1363));
    assert!(raw.len() <= MAX_URL_LEN);
    assert!(url::Url::parse(&raw).unwrap().as_str().len() > MAX_URL_LEN);

    let refused = cx.read(|cx| ext.open_url(&raw, cx));
    assert_eq!(refused, Err(ExternalOpenError::UrlTooLong));
    assert_error_hides_input(ExternalOpenError::UrlTooLong, &raw, &["example.com"]);
    assert_eq!(
        cx.opened_url(),
        None,
        "URL over MAX_URL_LEN after serialization must not reach GPUI"
    );
}

#[gpui::test]
fn refused_urls_return_expected_variant_and_do_not_open(cx: &mut TestAppContext) {
    let ext = extension(cx);
    assert_eq!(cx.opened_url(), None);

    let cases: &[(&str, ExternalOpenError, &[&str])] = &[
        ("", ExternalOpenError::InvalidUrl, &[]),
        (
            "https://example.com/a b",
            ExternalOpenError::InvalidUrl,
            &["example.com"],
        ),
        (
            "https://example.com/a\tb",
            ExternalOpenError::InvalidUrl,
            &["example.com"],
        ),
        (
            "https://example.com/a\rb",
            ExternalOpenError::InvalidUrl,
            &["example.com"],
        ),
        (
            "https://example.com/a\nb",
            ExternalOpenError::InvalidUrl,
            &["example.com"],
        ),
        (
            "https://example.com/a\0b",
            ExternalOpenError::InvalidUrl,
            &["example.com"],
        ),
        (
            "https://example.com/a\\b",
            ExternalOpenError::InvalidUrl,
            &["example.com"],
        ),
        (
            "https://example.com/<tag>",
            ExternalOpenError::InvalidUrl,
            &["example.com", "<tag>"],
        ),
        (
            "https://example.com/\"quoted\"",
            ExternalOpenError::InvalidUrl,
            &["example.com", "quoted"],
        ),
        (
            "https://example.com/%zz",
            ExternalOpenError::InvalidUrl,
            &["example.com", "%zz"],
        ),
        (
            "https://example.com/a>b",
            ExternalOpenError::InvalidUrl,
            &["example.com"],
        ),
        (
            "https://example.com/{x}",
            ExternalOpenError::InvalidUrl,
            &["example.com"],
        ),
        (
            "https://example.com/a|b",
            ExternalOpenError::InvalidUrl,
            &["example.com"],
        ),
        (
            "https://example.com/a^b",
            ExternalOpenError::InvalidUrl,
            &["example.com"],
        ),
        (
            "https://example.com/`cmd`",
            ExternalOpenError::InvalidUrl,
            &["example.com", "cmd"],
        ),
        (
            "https://example.com/%2",
            ExternalOpenError::InvalidUrl,
            &["example.com", "%2"],
        ),
        (
            "example.com/no-scheme",
            ExternalOpenError::InvalidUrl,
            &["example.com", "no-scheme"],
        ),
        (
            "file:///etc/hosts",
            ExternalOpenError::SchemeNotAllowed,
            &["file://", "/etc/hosts"],
        ),
        (
            "javascript:alert(1)",
            ExternalOpenError::SchemeNotAllowed,
            &["javascript", "alert"],
        ),
        (
            "data:text/html,x",
            ExternalOpenError::SchemeNotAllowed,
            &["data:", "text/html"],
        ),
        (
            "vbscript:x",
            ExternalOpenError::SchemeNotAllowed,
            &["vbscript"],
        ),
        (
            "ms-msdt:x",
            ExternalOpenError::SchemeNotAllowed,
            &["ms-msdt"],
        ),
        (
            "search-ms:x",
            ExternalOpenError::SchemeNotAllowed,
            &["search-ms"],
        ),
        (
            "ftp://example.com/",
            ExternalOpenError::SchemeNotAllowed,
            &["ftp://", "example.com"],
        ),
        (
            "yard://ext/x",
            ExternalOpenError::SchemeNotAllowed,
            &["yard://"],
        ),
        (
            "https://user:pass@example.com/",
            ExternalOpenError::InvalidUrl,
            &["user", "pass", "example.com"],
        ),
        (
            "https://user@example.com/",
            ExternalOpenError::InvalidUrl,
            &["user", "example.com"],
        ),
        (
            "http:///path",
            ExternalOpenError::InvalidUrl,
            &["http://", "/path"],
        ),
        ("mailto:", ExternalOpenError::InvalidUrl, &["mailto:"]),
        (
            "mailto://x@example.com",
            ExternalOpenError::InvalidUrl,
            &["mailto:", "x@example.com", "example.com"],
        ),
        (
            "https://example.com/a%00b",
            ExternalOpenError::InvalidUrl,
            &["example.com", "%00"],
        ),
        (
            "https://example.com/a%0ab",
            ExternalOpenError::InvalidUrl,
            &["example.com", "%0a"],
        ),
        (
            "https://example.com/a%7f",
            ExternalOpenError::InvalidUrl,
            &["example.com", "%7f"],
        ),
        (
            "mailto:a@example.com?subject=hi%0d%0aBcc:x@example.com",
            ExternalOpenError::InvalidUrl,
            &["a@example.com", "Bcc", "x@example.com"],
        ),
        (
            "mailto:not-an-email",
            ExternalOpenError::InvalidUrl,
            &["not-an-email"],
        ),
        ("mailto:%20", ExternalOpenError::InvalidUrl, &["%20"]),
        ("mailto:,", ExternalOpenError::InvalidUrl, &["mailto:"]),
        (
            "mailto:a@example.com,",
            ExternalOpenError::InvalidUrl,
            &["a@example.com"],
        ),
        (
            "mailto:@example.com",
            ExternalOpenError::InvalidUrl,
            &["@example.com"],
        ),
        ("mailto:a@", ExternalOpenError::InvalidUrl, &["a@"]),
        (
            "https://example.com/a[0]",
            ExternalOpenError::InvalidUrl,
            &["example.com", "[0]"],
        ),
        (
            "mailto:a@example.com?subject=[YARD]",
            ExternalOpenError::InvalidUrl,
            &["a@example.com", "[YARD]"],
        ),
        ("https://:80/", ExternalOpenError::InvalidUrl, &[":80"]),
        (
            "https://@example.com/",
            ExternalOpenError::InvalidUrl,
            &["example.com"],
        ),
        (
            "https://:pass@example.com/",
            ExternalOpenError::InvalidUrl,
            &["pass", "example.com"],
        ),
        (
            "https:example.com",
            ExternalOpenError::InvalidUrl,
            &["example.com"],
        ),
        (
            "https:/example.com",
            ExternalOpenError::InvalidUrl,
            &["example.com"],
        ),
    ];

    for &(input, expected, needles) in cases {
        let result = cx.read(|cx| ext.open_url(input, cx));
        assert_eq!(
            result,
            Err(expected),
            "expected `{input:?}` to be refused with {expected:?}"
        );
        assert_error_hides_input(expected, input, needles);
        assert_eq!(
            cx.opened_url(),
            None,
            "refused URL `{input:?}` must not reach GPUI"
        );
    }
}

#[gpui::test]
fn refused_mailto_recipients_return_invalid_url(cx: &mut TestAppContext) {
    let ext = extension(cx);
    assert_eq!(cx.opened_url(), None);

    let cases: &[(&str, &[&str])] = &[
        ("mailto:%20@%20", &["%20"]),
        ("mailto:%20@example.com", &["example.com"]),
        ("mailto:a@%20", &["%20"]),
        ("mailto:a%20b@example.com", &["example.com"]),
        ("mailto:a%40example.com", &["example.com"]),
        ("mailto:a%40b@example.com", &["example.com"]),
        ("mailto:a@b@c", &["a@b@c"]),
        ("mailto:a@%FF%FE.example", &["%FF%FE", "example"]),
        // Contract: the part before `?` must hold at least one recipient.
        ("mailto:?to=a@example.com", &["a@example.com"]),
    ];

    for &(input, needles) in cases {
        let result = cx.read(|cx| ext.open_url(input, cx));
        assert_eq!(
            result,
            Err(ExternalOpenError::InvalidUrl),
            "expected `{input:?}` to be refused with InvalidUrl"
        );
        assert_error_hides_input(ExternalOpenError::InvalidUrl, input, needles);
        assert_eq!(
            cx.opened_url(),
            None,
            "refused URL `{input:?}` must not reach GPUI"
        );
    }
}

#[gpui::test]
fn refused_mailto_query_recipients_return_invalid_url(cx: &mut TestAppContext) {
    let ext = extension(cx);
    assert_eq!(cx.opened_url(), None);

    let cases: &[(&str, &[&str])] = &[
        (
            "mailto:a@example.com?to=not-an-email",
            &["a@example.com", "not-an-email"],
        ),
        ("mailto:a@example.com?cc=%20@x", &["a@example.com", "%20@x"]),
        (
            "mailto:a@example.com?bcc=not-an-email",
            &["a@example.com", "not-an-email"],
        ),
    ];

    for &(input, needles) in cases {
        let result = cx.read(|cx| ext.open_url(input, cx));
        assert_eq!(
            result,
            Err(ExternalOpenError::InvalidUrl),
            "expected `{input:?}` to be refused with InvalidUrl"
        );
        assert_error_hides_input(ExternalOpenError::InvalidUrl, input, needles);
        assert_eq!(
            cx.opened_url(),
            None,
            "refused URL `{input:?}` must not reach GPUI"
        );
    }
}

#[gpui::test]
fn refused_mailto_repeated_fields_return_invalid_url(cx: &mut TestAppContext) {
    let ext = extension(cx);
    assert_eq!(cx.opened_url(), None);

    let cases: &[(&str, &[&str])] = &[
        ("mailto:a@example.com?body=x&BODY=y", &["a@example.com"]),
        (
            "mailto:a@example.com?subject=a&subject=b",
            &["a@example.com", "subject"],
        ),
    ];

    for &(input, needles) in cases {
        let result = cx.read(|cx| ext.open_url(input, cx));
        assert_eq!(
            result,
            Err(ExternalOpenError::InvalidUrl),
            "expected `{input:?}` to be refused with InvalidUrl"
        );
        assert_error_hides_input(ExternalOpenError::InvalidUrl, input, needles);
        assert_eq!(
            cx.opened_url(),
            None,
            "refused URL `{input:?}` must not reach GPUI"
        );
    }
}

#[gpui::test]
fn refused_mailto_escapes_and_fragment_return_invalid_url(cx: &mut TestAppContext) {
    let ext = extension(cx);
    assert_eq!(cx.opened_url(), None);

    let cases: &[(&str, &[&str])] = &[
        ("mailto:a@example.com?bo%0Ddy=x", &["a@example.com", "%0D"]),
        ("mailto:a@example.com?body=a%00b", &["a@example.com", "%00"]),
        ("mailto:a@example.com?body=a%1Fb", &["a@example.com", "%1F"]),
        ("mailto:a@example.com?body=a%7Fb", &["a@example.com", "%7F"]),
        (
            "mailto:a@example.com?body=x#a%0Ab",
            &["a@example.com", "%0A"],
        ),
        (
            "mailto:a@example.com?subject=a%0Ab",
            &["a@example.com", "subject", "%0A"],
        ),
    ];

    for &(input, needles) in cases {
        let result = cx.read(|cx| ext.open_url(input, cx));
        assert_eq!(
            result,
            Err(ExternalOpenError::InvalidUrl),
            "expected `{input:?}` to be refused with InvalidUrl"
        );
        assert_error_hides_input(ExternalOpenError::InvalidUrl, input, needles);
        assert_eq!(
            cx.opened_url(),
            None,
            "refused URL `{input:?}` must not reach GPUI"
        );
    }
}

#[gpui::test]
fn refused_http_and_https_escapes_and_brackets_return_invalid_url(cx: &mut TestAppContext) {
    let ext = extension(cx);
    assert_eq!(cx.opened_url(), None);

    let cases: &[(&str, &[&str])] = &[
        (
            "https://example.com/?body=a%0D%0A",
            &["example.com", "%0D%0A"],
        ),
        ("https://[::1]/a[0]", &["[::1]", "[0]"]),
        ("https://[::1]/?a[0]=1", &["[::1]", "[0]"]),
        ("https://[::1]:[80]/", &["[::1]", "[80]"]),
        ("https://[::1]@example.com/", &["[::1]", "example.com"]),
        ("https://user@[::1]/", &["user", "[::1]"]),
    ];

    for &(input, needles) in cases {
        let result = cx.read(|cx| ext.open_url(input, cx));
        assert_eq!(
            result,
            Err(ExternalOpenError::InvalidUrl),
            "expected `{input:?}` to be refused with InvalidUrl"
        );
        assert_error_hides_input(ExternalOpenError::InvalidUrl, input, needles);
        assert_eq!(
            cx.opened_url(),
            None,
            "refused URL `{input:?}` must not reach GPUI"
        );
    }
}

#[gpui::test]
fn refused_ftp_ipv6_url_returns_scheme_not_allowed(cx: &mut TestAppContext) {
    let ext = extension(cx);
    assert_eq!(cx.opened_url(), None);

    let input = "ftp://[::1]/";
    let result = cx.read(|cx| ext.open_url(input, cx));
    assert_eq!(
        result,
        Err(ExternalOpenError::SchemeNotAllowed),
        "expected `{input}` to be refused with SchemeNotAllowed"
    );
    assert_error_hides_input(
        ExternalOpenError::SchemeNotAllowed,
        input,
        &["ftp", "[::1]"],
    );
    assert_eq!(
        cx.opened_url(),
        None,
        "refused URL `{input}` must not reach GPUI"
    );
}

#[gpui::test]
fn refused_paths_for_reveal_and_open_return_expected_variant(cx: &mut TestAppContext) {
    let ext = extension(cx);
    let dir = tempfile::tempdir().unwrap();

    let missing = dir.path().join("missing-document.txt");
    let with_control = dir.path().join("bad\ncontrol.txt");
    let with_nul = dir.path().join("bad\x01control.txt");
    let relative = Path::new("relative/document.txt");
    let empty = Path::new("");

    let shared_cases: &[(&Path, ExternalOpenError, &[&str])] = &[
        (
            relative,
            ExternalOpenError::InvalidPath,
            &["relative", "document.txt"],
        ),
        (empty, ExternalOpenError::InvalidPath, &[]),
        (
            &missing,
            ExternalOpenError::NotFound,
            &["missing-document.txt"],
        ),
        (
            &with_control,
            ExternalOpenError::InvalidPath,
            &["control.txt"],
        ),
        (&with_nul, ExternalOpenError::InvalidPath, &["control.txt"]),
    ];

    for &(path, expected, needles) in shared_cases {
        let path_text = path.to_string_lossy();

        let reveal = cx.read(|cx| ext.reveal_path(path, cx));
        assert_eq!(
            reveal,
            Err(expected),
            "reveal_path({path:?}) should return {expected:?}"
        );
        assert_error_hides_input(expected, &path_text, needles);

        let open = cx.read(|cx| ext.open_path(path, cx));
        assert_eq!(
            open,
            Err(expected),
            "open_path({path:?}) should return {expected:?}"
        );
        assert_error_hides_input(expected, &path_text, needles);
    }

    let root = dir.path().ancestors().last().unwrap();
    #[cfg(unix)]
    assert_eq!(root, Path::new("/"));

    let reveal_root = cx.read(|cx| ext.reveal_path(root, cx));
    assert_eq!(
        reveal_root,
        Err(ExternalOpenError::InvalidPath),
        "reveal_path({root:?}) must refuse a filesystem root"
    );
    assert_error_hides_input(ExternalOpenError::InvalidPath, &root.to_string_lossy(), &[]);

    let open_root = cx.read(|cx| ext.open_path(root, cx));
    assert_eq!(
        open_root,
        Err(ExternalOpenError::InvalidPath),
        "open_path({root:?}) must refuse a filesystem root"
    );
    assert_error_hides_input(ExternalOpenError::InvalidPath, &root.to_string_lossy(), &[]);
}

#[gpui::test]
fn refused_parent_dir_paths_for_reveal_and_open_return_invalid_path(cx: &mut TestAppContext) {
    let ext = extension(cx);
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("a");
    fs::create_dir(&sub).unwrap();

    let traversal = sub.join("..").join("..");
    let root_parent = Path::new("/..");

    for path in [traversal.as_path(), root_parent] {
        let path_text = path.to_string_lossy();

        let reveal = cx.read(|cx| ext.reveal_path(path, cx));
        assert_eq!(
            reveal,
            Err(ExternalOpenError::InvalidPath),
            "reveal_path({path:?}) must refuse a `..` component"
        );
        assert_error_hides_input(ExternalOpenError::InvalidPath, &path_text, &[".."]);

        let open = cx.read(|cx| ext.open_path(path, cx));
        assert_eq!(
            open,
            Err(ExternalOpenError::InvalidPath),
            "open_path({path:?}) must refuse a `..` component"
        );
        assert_error_hides_input(ExternalOpenError::InvalidPath, &path_text, &[".."]);
    }
}

#[gpui::test]
fn refused_cur_dir_paths_for_reveal_and_open_return_invalid_path(cx: &mut TestAppContext) {
    let ext = extension(cx);
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("notes.txt");
    fs::write(&plain, "plain text").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&plain, fs::Permissions::from_mode(0o644)).unwrap();
    }

    let sep = std::path::MAIN_SEPARATOR;
    let with_cur_dir = PathBuf::from(format!("{}{sep}.{sep}notes.txt", dir.path().display()));
    let path_text = with_cur_dir.to_string_lossy();

    let reveal = cx.read(|cx| ext.reveal_path(&with_cur_dir, cx));
    assert_eq!(
        reveal,
        Err(ExternalOpenError::InvalidPath),
        "reveal_path({with_cur_dir:?}) must refuse a `.` segment"
    );
    assert_error_hides_input(ExternalOpenError::InvalidPath, &path_text, &["notes.txt"]);

    let open = cx.read(|cx| ext.open_path(&with_cur_dir, cx));
    assert_eq!(
        open,
        Err(ExternalOpenError::InvalidPath),
        "open_path({with_cur_dir:?}) must refuse a `.` segment"
    );
    assert_error_hides_input(ExternalOpenError::InvalidPath, &path_text, &["notes.txt"]);
}

#[cfg(unix)]
#[gpui::test]
fn refused_symlinks_and_non_utf8_paths_for_reveal_and_open(cx: &mut TestAppContext) {
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::os::unix::fs::symlink;

    let ext = extension(cx);
    let dir = tempfile::tempdir().unwrap();

    // Symlink to `/` -> InvalidPath for both methods.
    let link_to_root = dir.path().join("link-to-root");
    symlink("/", &link_to_root).unwrap();

    // Symlink whose target name holds a control character -> InvalidPath for both methods.
    let control_target = dir.path().join("target\x01control.txt");
    fs::write(&control_target, "plain text").unwrap();
    let link_to_control = dir.path().join("clean-link.txt");
    symlink(&control_target, &link_to_control).unwrap();

    // Non-UTF-8 path -> InvalidPath for both methods.
    let mut non_utf8_bytes = dir.path().as_os_str().as_bytes().to_vec();
    non_utf8_bytes.extend_from_slice(b"/non-utf8-\xff.txt");
    let non_utf8 = PathBuf::from(OsString::from_vec(non_utf8_bytes));

    // Dangling symlink -> NotFound for both methods.
    let missing_target = dir.path().join("missing-target.txt");
    let dangling = dir.path().join("dangling-link.txt");
    symlink(&missing_target, &dangling).unwrap();

    let cases: &[(&Path, ExternalOpenError, &[&str])] = &[
        (
            &link_to_root,
            ExternalOpenError::InvalidPath,
            &["link-to-root"],
        ),
        (
            &link_to_control,
            ExternalOpenError::InvalidPath,
            &["clean-link.txt", "control.txt"],
        ),
        (&non_utf8, ExternalOpenError::InvalidPath, &["non-utf8"]),
        (
            &dangling,
            ExternalOpenError::NotFound,
            &["dangling-link.txt", "missing-target.txt"],
        ),
    ];

    for &(path, expected, needles) in cases {
        let path_text = path.to_string_lossy();

        let reveal = cx.read(|cx| ext.reveal_path(path, cx));
        assert_eq!(
            reveal,
            Err(expected),
            "reveal_path({path:?}) should return {expected:?}"
        );
        assert_error_hides_input(expected, &path_text, needles);

        let open = cx.read(|cx| ext.open_path(path, cx));
        assert_eq!(
            open,
            Err(expected),
            "open_path({path:?}) should return {expected:?}"
        );
        assert_error_hides_input(expected, &path_text, needles);
    }
}

#[cfg(unix)]
#[gpui::test]
fn reveal_path_refuses_fifo_without_opening_it(cx: &mut TestAppContext) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let ext = extension(cx);
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("pipe.fifo");
    let c_path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: `c_path` is a valid NUL-terminated path passed to `mkfifo`.
    let rc = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
    assert_eq!(rc, 0, "mkfifo failed: {}", std::io::Error::last_os_error());

    let result = cx.read(|cx| ext.reveal_path(&fifo, cx));
    assert_eq!(
        result,
        Err(ExternalOpenError::NotAFile),
        "reveal_path must refuse a FIFO"
    );
    assert_error_hides_input(
        ExternalOpenError::NotAFile,
        &fifo.to_string_lossy(),
        &["pipe.fifo"],
    );
}

#[cfg(unix)]
#[gpui::test]
fn open_path_refuses_fifo_without_opening_it(cx: &mut TestAppContext) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let ext = extension(cx);
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("open-pipe.fifo");
    let c_path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: `c_path` is a valid NUL-terminated path passed to `mkfifo`.
    let rc = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
    assert_eq!(rc, 0, "mkfifo failed: {}", std::io::Error::last_os_error());

    let result = cx.read(|cx| ext.open_path(&fifo, cx));
    assert_eq!(
        result,
        Err(ExternalOpenError::NotAFile),
        "open_path must refuse a FIFO"
    );
    assert_error_hides_input(
        ExternalOpenError::NotAFile,
        &fifo.to_string_lossy(),
        &["open-pipe.fifo"],
    );
}

#[cfg(windows)]
#[gpui::test]
fn windows_invalid_paths_are_refused_for_reveal_and_open(cx: &mut TestAppContext) {
    let ext = extension(cx);

    // Mapped drives cannot be created on a runner; left to the lead's review.
    let cases: &[(&str, &[&str])] = &[
        (r"\\server\share\x.txt", &["server", "share", "x.txt"]),
        (r"\\?\C:\x.txt", &["x.txt"]),
        (r"\\?\UNC\server\share\x.txt", &["server", "share", "x.txt"]),
        (r"\\.\C:\x.txt", &["x.txt"]),
        (r"C:\tmp\trailing.\x.txt", &["trailing.", "x.txt"]),
        (r"C:\tmp\trailing \x.txt", &["trailing ", "x.txt"]),
        (r"C:\tmp\file.", &["file."]),
        (r"C:\tmp\file ", &["file "]),
        (r"C:\tmp\CON.txt", &["CON.txt"]),
        (r"C:\tmp\nul", &["nul"]),
        // Superscript DOS device names COM¹–COM³ and LPT¹–LPT³.
        ("C:\\tmp\\COM\u{00B9}", &["COM\u{00B9}"]),
        ("C:\\tmp\\COM\u{00B2}", &["COM\u{00B2}"]),
        ("C:\\tmp\\COM\u{00B3}", &["COM\u{00B3}"]),
        ("C:\\tmp\\LPT\u{00B9}", &["LPT\u{00B9}"]),
        ("C:\\tmp\\LPT\u{00B2}.txt", &["LPT\u{00B2}.txt"]),
        ("C:\\tmp\\LPT\u{00B3}", &["LPT\u{00B3}"]),
        (r"C:\tmp\COM0", &["COM0"]),
        (r"C:\tmp\LPT9", &["LPT9"]),
        (r"C:\tmp\CONIN$", &["CONIN$"]),
        (r"C:\tmp\CONOUT$", &["CONOUT$"]),
        (r"C:\tmp\CON:stream", &["CON:stream"]),
        (r"C:\tmp\notes.txt:stream", &["notes.txt:stream"]),
        (r"C:\tmp\notes.txt::$DATA", &["notes.txt::$DATA"]),
        (r"C:\tmp\foo.:stream", &["foo.:stream"]),
        (r"C:\tmp\dir:ads\notes.txt", &["dir:ads", "notes.txt"]),
        (r"C:\tmp\.\notes.txt", &["notes.txt"]),
    ];

    for &(raw, needles) in cases {
        let path = Path::new(raw);

        let reveal = cx.read(|cx| ext.reveal_path(path, cx));
        assert_eq!(
            reveal,
            Err(ExternalOpenError::InvalidPath),
            "reveal_path({path:?}) must refuse invalid Windows path"
        );
        assert_error_hides_input(ExternalOpenError::InvalidPath, raw, needles);

        let open = cx.read(|cx| ext.open_path(path, cx));
        assert_eq!(
            open,
            Err(ExternalOpenError::InvalidPath),
            "open_path({path:?}) must refuse invalid Windows path"
        );
        assert_error_hides_input(ExternalOpenError::InvalidPath, raw, needles);
    }
}

#[gpui::test]
fn open_path_refuses_folders_and_executables(cx: &mut TestAppContext) {
    let ext = extension(cx);
    let dir = tempfile::tempdir().unwrap();

    // Folder -> NotAFile (including a folder named `x.app`).
    let folder = dir.path().join("sample-folder");
    fs::create_dir(&folder).unwrap();
    let app_bundle = folder.join("x.app");
    fs::create_dir(&app_bundle).unwrap();
    for (folder_path, needle) in [(&folder, "sample-folder"), (&app_bundle, "x.app")] {
        let folder_result = cx.read(|cx| ext.open_path(folder_path, cx));
        assert_eq!(
            folder_result,
            Err(ExternalOpenError::NotAFile),
            "open_path({folder_path:?}) must refuse a folder with NotAFile"
        );
        assert_error_hides_input(
            ExternalOpenError::NotAFile,
            &folder_path.to_string_lossy(),
            &[needle],
        );
    }

    // Blocked extensions as plain text files without an execute bit -> Executable.
    for name in [
        "x.sh",
        "x.EXE",
        "x.app",
        "x.application",
        "x.terminal",
        "x.webloc",
        "x.inetloc",
        "x.msix",
        "x.py",
        "x.txt.exe",
    ] {
        let file = dir.path().join(name);
        fs::write(&file, "plain text").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        }
        let result = cx.read(|cx| ext.open_path(&file, cx));
        assert_eq!(
            result,
            Err(ExternalOpenError::Executable),
            "open_path({name}) must refuse blocked extension even without execute bit"
        );
        assert_error_hides_input(
            ExternalOpenError::Executable,
            &file.to_string_lossy(),
            &[name],
        );
    }

    // Unix-only: execute bit and symlink to an executable file -> Executable.
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let exec_file = dir.path().join("plain-with-exec.txt");
        fs::write(&exec_file, "hello").unwrap();
        fs::set_permissions(&exec_file, fs::Permissions::from_mode(0o755)).unwrap();
        let exec_result = cx.read(|cx| ext.open_path(&exec_file, cx));
        assert_eq!(
            exec_result,
            Err(ExternalOpenError::Executable),
            "open_path must refuse a file with an execute bit"
        );
        assert_error_hides_input(
            ExternalOpenError::Executable,
            &exec_file.to_string_lossy(),
            &["plain-with-exec.txt"],
        );

        let link = dir.path().join("link.txt");
        symlink(&exec_file, &link).unwrap();
        let link_result = cx.read(|cx| ext.open_path(&link, cx));
        assert_eq!(
            link_result,
            Err(ExternalOpenError::Executable),
            "open_path must refuse a .txt symlink pointing to an executable file"
        );
        assert_error_hides_input(
            ExternalOpenError::Executable,
            &link.to_string_lossy(),
            &["link.txt", "plain-with-exec.txt"],
        );

        // Symlink named `x.sh` pointing at a plain `.txt` file -> Executable (judged on given name).
        let link_dir = dir.path().join("links");
        fs::create_dir(&link_dir).unwrap();
        let plain_target = link_dir.join("plain-target.txt");
        fs::write(&plain_target, "plain text").unwrap();
        fs::set_permissions(&plain_target, fs::Permissions::from_mode(0o644)).unwrap();
        let sh_link = link_dir.join("x.sh");
        symlink(&plain_target, &sh_link).unwrap();
        let sh_link_result = cx.read(|cx| ext.open_path(&sh_link, cx));
        assert_eq!(
            sh_link_result,
            Err(ExternalOpenError::Executable),
            "open_path must refuse a .sh symlink pointing to a plain .txt file"
        );
        assert_error_hides_input(
            ExternalOpenError::Executable,
            &sh_link.to_string_lossy(),
            &["x.sh", "plain-target.txt"],
        );

        // Group-only execute bit, other-only execute bit, and extensionless +x -> Executable.
        for (name, mode) in [
            ("group-exec.txt", 0o654),
            ("other-exec.txt", 0o645),
            ("extensionless-runnable", 0o755),
        ] {
            let file = dir.path().join(name);
            fs::write(&file, "hello").unwrap();
            fs::set_permissions(&file, fs::Permissions::from_mode(mode)).unwrap();
            let result = cx.read(|cx| ext.open_path(&file, cx));
            assert_eq!(
                result,
                Err(ExternalOpenError::Executable),
                "open_path({name}, mode {mode:#o}) must refuse any execute bit"
            );
            assert_error_hides_input(
                ExternalOpenError::Executable,
                &file.to_string_lossy(),
                &[name],
            );
        }
    }
}
