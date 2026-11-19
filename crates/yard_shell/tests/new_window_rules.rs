use gpui::{AppContext, IntoElement, TestAppContext, px};
use std::{cell::RefCell, rc::Rc};
use yard_shell as shell;

type Handle = gpui::WindowHandle<shell::Shell>;
type Opened = Rc<RefCell<Vec<Handle>>>;

struct Card;
impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
    }
}

fn setup(app: &mut TestAppContext) -> (shell::Extension, Opened) {
    let opened = Opened::default();
    let recorded = opened.clone();
    let extension = app.update(|cx| {
        yard_ui::init(cx);
        let extension = shell::Extension::new(cx, "tour").unwrap();
        extension.register(cx, "tour.card", &[], |_, _, _, cx| cx.new(|_| Card).into());
        extension.place_root(cx, "root", "yard.row");
        extension.place(cx, "column", "yard.column", "root", "children");
        extension.place(cx, "tabs", "yard.tabs", "column", "children");
        extension.place(cx, "leaf", "tour.card", "tabs", "children");
        extension.place(cx, "other", "tour.card", "tabs", "children");
        shell::set_window_opener(cx, move |cx| {
            let handle = cx
                .open_window(Default::default(), shell::new_shell)
                .unwrap();
            handle
                .update(cx, |_, window, _| window.activate_window())
                .unwrap();
            recorded.borrow_mut().push(handle);
        });
        extension
    });
    (extension, opened)
}

fn draw(handle: Handle, app: &mut TestAppContext) {
    app.simulate_window_resize(handle.into(), gpui::size(px(900.), px(600.)));
    gpui::AnyWindowHandle::from(handle)
        .update(app, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn open(app: &mut TestAppContext) -> Handle {
    let handle = app.update(|cx| {
        cx.open_window(Default::default(), shell::new_shell)
            .unwrap()
    });
    draw(handle, app);
    handle
}

#[gpui::test]
fn new_window_from_the_second_window_opens_a_third(cx: &mut TestAppContext) {
    let mut app = cx.new_app();
    let (_, opened) = setup(&mut app);
    let main = open(&mut app);
    app.dispatch_action(main.into(), shell::NewWindow);
    let second = opened.borrow()[0];
    draw(second, &mut app);
    app.dispatch_action(second.into(), shell::NewWindow);
    assert_eq!(opened.borrow().len(), 2);
    assert_eq!(app.update(|cx| cx.windows().len()), 3);
    let third = opened.borrow()[1];
    assert_eq!(
        app.update(|cx| cx.active_window().unwrap().window_id()),
        gpui::AnyWindowHandle::from(third).window_id()
    );
}

#[gpui::test]
fn after_main_closes_new_windows_keep_their_layers_in_memory(cx: &mut TestAppContext) {
    let mut app = cx.new_app();
    let (extension, opened) = setup(&mut app);
    let directory =
        std::env::temp_dir().join(format!("yard-new-window-rules-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("layout.json");
    app.update(|cx| shell::set_layout_file(cx, file.clone()));
    let main = open(&mut app);
    app.update(|cx| extension.set_hidden(cx, "other", true));
    let saved = std::fs::read(&file).unwrap();
    app.dispatch_action(main.into(), shell::NewWindow);
    let second = opened.borrow()[0];
    draw(second, &mut app);
    main.update(&mut app, |_, window, _| window.remove_window())
        .unwrap();
    app.run_until_parked();
    app.dispatch_action(second.into(), shell::NewWindow);
    let third = opened.borrow()[1];
    draw(third, &mut app);
    app.update(|cx| {
        assert!(!shell::is_hidden(cx, "other"));
        assert_eq!(shell::user_layer_len(cx), 0);
        extension.set_hidden(cx, "leaf", true);
        assert_eq!(shell::user_layer_len(cx), 1);
    });
    assert_eq!(std::fs::read(&file).unwrap(), saved);
    assert_eq!(app.update(|cx| cx.windows().len()), 2);
    std::fs::remove_dir_all(directory).unwrap();
}

#[gpui::test]
fn new_window_bubbles_from_a_node_deep_in_the_tree(cx: &mut TestAppContext) {
    let mut app = cx.new_app();
    let (_, opened) = setup(&mut app);
    let main = open(&mut app);
    main.update(&mut app, |_, window, cx| {
        shell::focus_handle(cx, "leaf").unwrap().focus(window, cx);
    })
    .unwrap();
    draw(main, &mut app);
    main.update(&mut app, |_, window, cx| {
        assert_eq!(shell::focused_node(window, cx).unwrap().as_str(), "leaf");
    })
    .unwrap();
    app.dispatch_action(main.into(), shell::NewWindow);
    assert_eq!(opened.borrow().len(), 1);
    assert_eq!(app.update(|cx| cx.windows().len()), 2);
}

#[gpui::test]
fn new_window_uses_gpuis_cascade_from_the_active_window(cx: &mut TestAppContext) {
    let mut app = cx.new_app();
    let (_, opened) = setup(&mut app);
    let main = app.update(|cx| {
        cx.open_window(
            gpui::WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::new(
                    gpui::point(px(100.), px(140.)),
                    gpui::size(px(900.), px(600.)),
                ))),
                ..Default::default()
            },
            shell::new_shell,
        )
        .unwrap()
    });
    draw(main, &mut app);
    main.update(&mut app, |_, window, _| window.activate_window())
        .unwrap();
    app.run_until_parked();
    app.dispatch_action(main.into(), shell::NewWindow);
    let second = opened.borrow()[0];
    let actual = second
        .update(&mut app, |_, window, _| window.window_bounds())
        .unwrap();
    assert_eq!(
        actual,
        gpui::WindowBounds::Windowed(gpui::Bounds::new(
            gpui::point(px(125.), px(165.)),
            gpui::size(px(900.), px(600.)),
        ))
    );
}
