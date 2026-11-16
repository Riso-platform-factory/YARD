#![cfg(feature = "dev")]
mod support;
use gpui::TestAppContext;
use riso_shell::dev as shell;

#[gpui::test]
fn panel_lists_every_node_with_depth_and_refreshes_hidden_rows(cx: &mut TestAppContext) {
    if !support::child("panel_lists_every_node_with_depth_and_refreshes_hidden_rows") {
        return;
    }
    let (host, window) = support::setup(cx);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    let nodes = cx.update(|cx| shell::inspect(cx));
    assert_eq!(nodes.len(), 5);
    for selector in [
        "INSPECTOR-ROW-tour_frame-depth-0-hidden-false",
        "INSPECTOR-ROW-root-depth-1-hidden-false",
        "INSPECTOR-ROW-a-depth-2-hidden-false",
        "INSPECTOR-ROW-b-depth-2-hidden-false",
        "INSPECTOR-ROW-inspector-depth-1-hidden-false",
    ] {
        assert!(
            visual.debug_bounds(selector).is_some(),
            "missing {selector}"
        );
    }
    let panel = visual
        .debug_bounds("INSPECTOR-ROW-tour_frame-depth-0-hidden-false")
        .unwrap();
    let count = visual.debug_bounds("INSPECTOR-COUNT").unwrap();
    assert!(
        count.right() <= panel.right(),
        "header count fits the panel"
    );
    for selector in [
        "INSPECTOR-END-tour_frame",
        "INSPECTOR-END-root",
        "INSPECTOR-END-a",
        "INSPECTOR-END-b",
        "INSPECTOR-END-inspector",
    ] {
        let end = visual.debug_bounds(selector).unwrap();
        assert!(end.right() <= panel.right(), "{selector} fits the panel");
    }
    let x = |visual: &mut gpui::VisualTestContext, id| visual.debug_bounds(id).unwrap().origin.x;
    let frame = x(&mut visual, "INSPECTOR-ID-tour_frame");
    let root = x(&mut visual, "INSPECTOR-ID-root");
    let a = x(&mut visual, "INSPECTOR-ID-a");
    assert!(frame < root && root < a, "rows indent by tree depth");
    cx.update(|cx| host.set_hidden(cx, "a", true));
    support::draw(window, cx);
    assert!(
        visual
            .debug_bounds("INSPECTOR-ROW-a-depth-2-hidden-true")
            .is_some()
    );
    assert!(
        visual
            .debug_bounds("INSPECTOR-ROW-a-depth-2-hidden-false")
            .is_none()
    );
}
