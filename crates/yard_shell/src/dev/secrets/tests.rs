use super::entry_names;

#[cfg(target_os = "macos")]
#[test]
fn macos_entry_names_follow_contract_and_separate_apps_and_extensions() {
    let notes = entry_names("vn.example.notes", "Notes", "notes", "api-token");
    assert_eq!(notes.service, "vn.example.notes");
    assert_eq!(notes.account, "notes/api-token");

    let other_app = entry_names("vn.example.tasks", "Tasks", "notes", "api-token");
    assert_eq!(other_app.service, "vn.example.tasks");
    assert_ne!(
        notes.service, other_app.service,
        "two identifiers must produce different macOS service names"
    );

    let other_ext = entry_names("vn.example.notes", "Notes", "calendar", "api-token");
    assert_eq!(other_ext.account, "calendar/api-token");
    assert_ne!(
        notes.account, other_ext.account,
        "two extensions must produce different macOS account names"
    );
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[test]
fn linux_entry_names_follow_contract_and_separate_apps_and_extensions() {
    use std::collections::BTreeMap;

    macro_rules! attrs_of {
        ($names:expr) => {{
            let attrs: BTreeMap<String, String> = $names
                .attributes
                .iter()
                .map(|(k, v)| {
                    (
                        AsRef::<str>::as_ref(k).to_owned(),
                        AsRef::<str>::as_ref(v).to_owned(),
                    )
                })
                .collect();
            attrs
        }};
    }

    let notes = entry_names("vn.example.notes", "Notes", "notes", "api-token");
    assert_eq!(notes.label, "Notes: notes/api-token");
    let notes_attrs = attrs_of!(&notes);
    assert_eq!(
        notes_attrs,
        BTreeMap::from([
            ("application".to_owned(), "vn.example.notes".to_owned()),
            ("extension".to_owned(), "notes".to_owned()),
            ("key".to_owned(), "api-token".to_owned()),
        ])
    );

    let other_app = entry_names("vn.example.tasks", "Tasks", "notes", "api-token");
    assert_eq!(other_app.label, "Tasks: notes/api-token");
    let other_app_attrs = attrs_of!(&other_app);
    assert_ne!(
        notes_attrs, other_app_attrs,
        "two identifiers must produce different Linux attributes"
    );
    assert_eq!(
        other_app_attrs.get("application").map(String::as_str),
        Some("vn.example.tasks")
    );

    let other_ext = entry_names("vn.example.notes", "Notes", "calendar", "api-token");
    assert_eq!(other_ext.label, "Notes: calendar/api-token");
    let other_ext_attrs = attrs_of!(&other_ext);
    assert_ne!(
        notes_attrs, other_ext_attrs,
        "two extensions must produce different Linux attributes"
    );
    assert_ne!(
        notes.label, other_ext.label,
        "two extensions must produce different Linux labels"
    );
}

#[cfg(target_os = "windows")]
#[test]
fn windows_entry_names_follow_contract_and_separate_apps_and_extensions() {
    let notes = entry_names("vn.example.notes", "Notes", "notes", "api-token");
    assert_eq!(notes.target, "vn.example.notes/notes/api-token");
    assert_eq!(notes.user_name, "notes");

    let other_app = entry_names("vn.example.tasks", "Tasks", "notes", "api-token");
    assert_eq!(other_app.target, "vn.example.tasks/notes/api-token");
    assert_ne!(
        notes.target, other_app.target,
        "two identifiers must produce different Windows targets"
    );

    let other_ext = entry_names("vn.example.notes", "Notes", "calendar", "api-token");
    assert_eq!(other_ext.target, "vn.example.notes/calendar/api-token");
    assert_eq!(other_ext.user_name, "calendar");
    assert_ne!(
        notes.target, other_ext.target,
        "two extensions must produce different Windows targets"
    );
    assert_ne!(
        notes.user_name, other_ext.user_name,
        "two extensions must produce different Windows user names"
    );
}
