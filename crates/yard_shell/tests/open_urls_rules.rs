use gpui::{App, TestAppContext};
use std::{cell::RefCell, rc::Rc};
use yard_shell as shell;

type Received = Rc<RefCell<Vec<String>>>;

fn record_urls(cx: &mut App, received: Received, nested: Option<&'static str>) {
    yard_ui::init(cx);
    shell::set_url_scheme(cx, "yard");
    let extension = shell::Extension::new(cx, "tour").unwrap();
    extension.on_open_url(cx, move |url, cx| {
        received.borrow_mut().push(url.to_owned());
        if url.ends_with("/first")
            && let Some(nested) = nested
        {
            shell::open_urls(cx, vec![nested.into()]);
        }
    });
}

#[gpui::test]
fn scheme_matching_is_ascii_case_insensitive(cx: &mut TestAppContext) {
    let app = cx.new_app();
    let received = Received::default();
    app.update(|cx| {
        record_urls(cx, received.clone(), None);
        shell::extensions_loaded(cx);
        shell::open_urls(cx, vec!["YARD://tour/hello".into()]);
    });
    assert_eq!(*received.borrow(), ["YARD://tour/hello"]);
}

#[gpui::test]
fn malformed_urls_do_not_reach_the_host_handler(cx: &mut TestAppContext) {
    let app = cx.new_app();
    let received = Received::default();
    let valid = "yard://tour:123/hello%20world?token=%7A";
    app.update(|cx| {
        record_urls(cx, received.clone(), None);
        shell::extensions_loaded(cx);
        shell::open_urls(
            cx,
            vec![
                "yard://tour:+1/hello".into(),
                "yard://tour/%zz".into(),
                valid.into(),
            ],
        );
    });
    assert_eq!(*received.borrow(), [valid]);
}

#[gpui::test]
fn a_handler_can_open_a_url_without_interrupting_its_batch(cx: &mut TestAppContext) {
    let app = cx.new_app();
    let received = Received::default();
    let first = "yard://tour/first";
    let second = "yard://tour/second";
    let nested = "yard://tour/nested";
    app.update(|cx| {
        record_urls(cx, received.clone(), Some(nested));
        shell::extensions_loaded(cx);
        shell::open_urls(cx, vec![first.into(), second.into()]);
    });
    assert_eq!(*received.borrow(), [first, second, nested]);
}

#[gpui::test]
fn a_url_arriving_during_handover_follows_every_earlier_url(cx: &mut TestAppContext) {
    let app = cx.new_app();
    let received = Received::default();
    let first = "yard://tour/first";
    let second = "yard://tour/second";
    let third = "yard://tour/third";
    let nested = "yard://tour/nested";
    app.update(|cx| {
        record_urls(cx, received.clone(), Some(nested));
        shell::open_urls(cx, vec![first.into(), second.into()]);
        shell::open_urls(cx, vec![third.into()]);
        assert!(received.borrow().is_empty());
        shell::extensions_loaded(cx);
    });
    assert_eq!(*received.borrow(), [first, second, third, nested]);
}
