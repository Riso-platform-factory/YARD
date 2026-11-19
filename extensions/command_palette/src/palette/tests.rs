use super::humanize_action_name;
use super::matching::{match_query, rank_candidates};

#[test]
fn humanize_formats_namespaces_underscores_words_and_acronyms() {
    assert_eq!(
        humanize_action_name("editor::GoToDefinition"),
        "editor: go to definition"
    );
    assert_eq!(
        humanize_action_name("editor::Backspace"),
        "editor: backspace"
    );
    assert_eq!(
        humanize_action_name("go_to_line::Toggle"),
        "go to line: toggle"
    );
    assert_eq!(
        humanize_action_name("yard::ToggleZoom"),
        "yard: toggle zoom"
    );
    assert_eq!(
        humanize_action_name("editor::FormatHTML"),
        "editor: format HTML"
    );
    assert_eq!(
        humanize_action_name("editor::HTMLFormat"),
        "editor: HTML format"
    );
    assert_eq!(
        humanize_action_name("zed::OpenURLInBrowser"),
        "zed: open URL in browser"
    );
}

#[test]
fn subsequence_matching_is_case_insensitive_and_rejects_non_subsequences() {
    assert!(match_query("tz", "yard: toggle zoom").is_some());
    assert!(match_query("TZ", "yard: toggle zoom").is_some());
    assert!(match_query("url", "zed: open URL").is_some());
    assert!(match_query("zt", "yard: toggle zoom").is_none());
    assert!(match_query("qeq", "palette test: quest").is_none());
}

#[test]
fn ranking_orders_word_starts_then_fewer_gaps_then_by_name() {
    let candidates = [
        "palette test: bequest",
        "palette test: quiet",
        "palette test: banquet",
        "palette test: quest",
        "yard: toggle zoom",
    ];
    let matched: Vec<&str> = rank_candidates("qe", &candidates)
        .into_iter()
        .map(|index| candidates[index])
        .collect();
    assert_eq!(
        matched,
        [
            "palette test: quest",
            "palette test: quiet",
            "palette test: banquet",
            "palette test: bequest",
        ]
    );
}

#[test]
fn empty_query_returns_every_command_by_name() {
    let candidates = ["yard: toggle zoom", "yard: focus down", "yard: hide"];
    let matched: Vec<&str> = rank_candidates("", &candidates)
        .into_iter()
        .map(|index| candidates[index])
        .collect();
    assert_eq!(
        matched,
        ["yard: focus down", "yard: hide", "yard: toggle zoom"]
    );
}

#[test]
fn optimal_subsequence_prefers_word_starts_and_tight_gaps_over_earlier_letters() {
    let candidates = ["ritual: tab size", "ritual: toggle zoom"];
    let matched: Vec<&str> = rank_candidates("tz", &candidates)
        .into_iter()
        .map(|index| candidates[index])
        .collect();
    assert_eq!(matched, ["ritual: toggle zoom", "ritual: tab size"]);
}
