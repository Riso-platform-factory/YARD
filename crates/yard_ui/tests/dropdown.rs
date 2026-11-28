#![cfg(feature = "dev")]

use gpui::{
    AppContext, FocusHandle, InteractiveElement, IntoElement, KeyBinding, KeyDownEvent, KeyUpEvent,
    Keystroke, Modifiers, ParentElement, PlatformInput, StatefulInteractiveElement, Styled,
    TestAppContext, VisualTestContext, div, point, px, size,
};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{Arc, Mutex, OnceLock},
};
use yard_ui::{Assets, Dropdown};

gpui::actions!(dropdown_test, [DismissHost]);

static TEST_LOCK: Mutex<()> = Mutex::new(());
static LOGGER_INIT: OnceLock<()> = OnceLock::new();
static DIAGNOSTICS: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_ui")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            DIAGNOSTICS
                .lock()
                .unwrap()
                .push(format!("{} {}", record.level(), record.args()));
        }
    }

    fn flush(&self) {}
}

fn init_logger() {
    LOGGER_INIT.get_or_init(|| {
        tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {});
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
}

#[derive(Clone, Default)]
struct AssetTrace(Arc<Mutex<Vec<String>>>);

#[derive(Default)]
struct PathVisitor(Option<String>);

impl tracing::field::Visit for PathVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "path" {
            self.0 = Some(value.to_owned());
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "path" {
            self.0 = Some(format!("{value:?}").trim_matches('"').to_owned());
        }
    }
}

impl tracing::Subscriber for AssetTrace {
    fn register_callsite(
        &self,
        _: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::sometimes()
    }

    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("yard_ui") {
            return;
        }
        let mut visitor = PathVisitor::default();
        event.record(&mut visitor);
        if let Some(path) = visitor.0 {
            self.0.lock().unwrap().push(path);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

struct DropdownAssetView {
    selected: Option<usize>,
}

impl gpui::Render for DropdownAssetView {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div().size_full().child(
            Dropdown::new(
                "sort",
                "Sort order",
                vec!["Name".into(), "Date".into()],
                self.selected,
            )
            .on_select(|_, _, _| {}),
        )
    }
}

struct InteractiveDropdown {
    origin: FocusHandle,
    selected: Option<usize>,
    options: Vec<gpui::SharedString>,
    chosen: Rc<Cell<Option<usize>>>,
    host_dismissals: Rc<Cell<usize>>,
    underlay_clicks: Option<Rc<Cell<usize>>>,
}

impl InteractiveDropdown {
    fn two_options(origin: FocusHandle, chosen: Rc<Cell<Option<usize>>>) -> Self {
        Self {
            origin,
            selected: Some(0),
            options: vec!["Name".into(), "Date".into()],
            chosen,
            host_dismissals: Rc::new(Cell::new(0)),
            underlay_clicks: None,
        }
    }
}

impl gpui::Render for InteractiveDropdown {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let chosen = self.chosen.clone();
        let selected = self.selected;
        let host_dismissals = self.host_dismissals.clone();
        let underlay = self.underlay_clicks.clone().map(|clicks| {
            div()
                .id("underlay")
                .debug_selector(|| "underlay".into())
                .absolute()
                .inset_0()
                .on_click(move |_, _, _| clicks.set(clicks.get() + 1))
        });
        div()
            .id("root")
            .key_context("Host")
            .on_action(move |_: &DismissHost, _, _| {
                host_dismissals.set(host_dismissals.get() + 1);
            })
            .relative()
            .size_full()
            .children(underlay)
            .child(
                div()
                    .id("origin")
                    .w(px(40.))
                    .h(px(24.))
                    .track_focus(&self.origin),
            )
            .child(
                Dropdown::new("sort", "Sort order", self.options.clone(), selected)
                    .on_select(move |index, _, _| chosen.set(Some(index))),
            )
    }
}

fn step_frames(visual: &mut VisualTestContext, frames: usize) {
    for _ in 0..frames {
        visual.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }
}

fn press_enter(visual: &mut VisualTestContext) {
    let keystroke = Keystroke::parse("enter").unwrap();
    visual.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    visual.simulate_event(KeyUpEvent { keystroke });
}

#[gpui::test]
fn dropdown_exposes_named_checked_options_and_selects_on_click(cx: &mut TestAppContext) {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init_logger();
    let trace = AssetTrace::default();

    tracing::subscriber::with_default(trace, || {
        tracing::callsite::rebuild_interest_cache();
        cx.update(yard_ui::init);
        let chosen = Rc::new(Cell::new(None));
        let chosen_for_view = chosen.clone();
        let window = cx.open_window(size(px(600.), px(400.)), move |_, cx| InteractiveDropdown {
            origin: cx.focus_handle(),
            selected: Some(1),
            options: vec!["Name".into(), "Date".into(), "Size".into()],
            chosen: chosen_for_view,
            host_dismissals: Rc::new(Cell::new(0)),
            underlay_clicks: None,
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        step_frames(&mut visual, 2);

        // GPUI's test platform at ddb90e5 never activates AccessKit (`is_a11y_active() == false`),
        // so `ComboBox` `expanded` without `toggled` is verified on the live OS tree; here we check
        // collapsed vs expanded deployment, named `ContextMenu` items, and the checked entry.
        visual.update(|window, _| {
            assert!(!window.is_a11y_active());
            assert!(window.debug_a11y_tree_json().is_none());
        });
        assert!(visual.debug_bounds("MENU_ITEM-Name").is_none());
        assert!(visual.debug_bounds("MENU_ITEM-Date").is_none());
        assert!(visual.debug_bounds("MENU_ITEM-Size").is_none());

        let trigger = visual
            .debug_bounds("sort")
            .expect("dropdown trigger bounds");
        visual.simulate_click(trigger.center(), Modifiers::default());
        step_frames(&mut visual, 3);

        assert!(
            visual.debug_bounds("MENU_ITEM-Name").is_some(),
            "open menu exposes named 'Name' option"
        );
        assert!(
            visual.debug_bounds("MENU_ITEM-Date").is_some(),
            "open menu exposes named 'Date' option"
        );
        assert!(
            visual.debug_bounds("MENU_ITEM-Size").is_some(),
            "open menu exposes named 'Size' option"
        );

        // `ContextMenu::select_toggled_or_first` highlights the checked `toggleable_entry`
        // (`"Date"`, index 1) when the menu opens, so Confirm immediately chooses index 1.
        visual.simulate_keystrokes("enter");
        step_frames(&mut visual, 2);
        assert_eq!(
            chosen.get(),
            Some(1),
            "opening the menu highlights the checked option ('Date')"
        );
        assert!(visual.debug_bounds("MENU_ITEM-Date").is_none());

        // Reopen and choose another named option by clicking it.
        let trigger = visual
            .debug_bounds("sort")
            .expect("dropdown trigger bounds");
        visual.simulate_click(trigger.center(), Modifiers::default());
        step_frames(&mut visual, 3);

        let size_option = visual
            .debug_bounds("MENU_ITEM-Size")
            .expect("open dropdown 'Size' option bounds");
        visual.simulate_click(size_option.center(), Modifiers::default());
        step_frames(&mut visual, 2);

        assert_eq!(chosen.get(), Some(2));
        assert!(visual.debug_bounds("MENU_ITEM-Size").is_none());
    });
}

#[gpui::test]
fn open_by_enter_then_dismiss_after_frame_returns_focus_to_trigger(cx: &mut TestAppContext) {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init_logger();
    let trace = AssetTrace::default();

    tracing::subscriber::with_default(trace, || {
        tracing::callsite::rebuild_interest_cache();
        cx.update(yard_ui::init);
        let chosen = Rc::new(Cell::new(None));
        let window = cx.open_window(size(px(600.), px(400.)), move |_, cx| {
            InteractiveDropdown::two_options(cx.focus_handle(), chosen)
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        step_frames(&mut visual, 2);

        let trigger_focus = visual.update(|window, cx| {
            window.focus_next(cx);
            window.focused(cx).expect("dropdown trigger receives focus")
        });
        step_frames(&mut visual, 1);

        // Open by Enter on the focused trigger and let the menu settle across frames.
        press_enter(&mut visual);
        step_frames(&mut visual, 3);
        assert!(
            visual.debug_bounds("MENU_ITEM-Name").is_some(),
            "Enter opens the dropdown menu"
        );
        assert_ne!(
            visual.update(|window, cx| window.focused(cx)),
            Some(trigger_focus.clone()),
            "open menu takes focus once deployed"
        );

        // Normal dismissal after the menu has settled returns focus to the trigger.
        visual.simulate_click(point(px(500.), px(350.)), Modifiers::default());
        step_frames(&mut visual, 2);
        assert!(
            visual.debug_bounds("MENU_ITEM-Name").is_none(),
            "clicking outside dismisses the menu"
        );
        assert_eq!(
            visual.update(|window, cx| window.focused(cx)),
            Some(trigger_focus),
            "normal dismissal returns focus to the trigger"
        );
    });
}

#[gpui::test]
fn open_and_close_three_times_from_two_focus_origins_restores_each_origin(cx: &mut TestAppContext) {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init_logger();
    let trace = AssetTrace::default();

    tracing::subscriber::with_default(trace, || {
        tracing::callsite::rebuild_interest_cache();
        cx.update(yard_ui::init);
        let chosen = Rc::new(Cell::new(None));
        let origin_cell: Rc<std::cell::RefCell<Option<FocusHandle>>> = Rc::default();
        let origin_for_view = origin_cell.clone();
        let window = cx.open_window(size(px(600.), px(400.)), move |_, cx| {
            let origin = cx.focus_handle();
            *origin_for_view.borrow_mut() = Some(origin.clone());
            InteractiveDropdown::two_options(origin, chosen)
        });
        let origin_focus = origin_cell.borrow().clone().unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        step_frames(&mut visual, 2);

        let trigger_focus = visual.update(|window, cx| {
            window.focus_next(cx);
            window.focused(cx).expect("dropdown trigger receives focus")
        });
        assert_ne!(
            origin_focus, trigger_focus,
            "the two focus origins must be distinct handles"
        );

        // Observe focus across three open/close cycles starting from two different focus origins.
        for (attempt, start_focus) in [
            (1, origin_focus.clone()),
            (2, trigger_focus),
            (3, origin_focus),
        ] {
            visual.update(|window, cx| window.focus(&start_focus, cx));
            step_frames(&mut visual, 1);
            assert_eq!(
                visual.update(|window, cx| window.focused(cx)),
                Some(start_focus.clone()),
                "opening #{attempt} starts with focus on its origin"
            );

            let trigger = visual
                .debug_bounds("sort")
                .expect("dropdown trigger bounds");
            visual.simulate_click(trigger.center(), Modifiers::default());
            step_frames(&mut visual, 3);
            assert!(
                visual.debug_bounds("MENU_ITEM-Name").is_some(),
                "opening #{attempt} deploys the menu"
            );
            assert_ne!(
                visual.update(|window, cx| window.focused(cx)),
                Some(start_focus.clone()),
                "opening #{attempt} moves focus into the open menu"
            );

            visual.simulate_click(point(px(500.), px(350.)), Modifiers::default());
            step_frames(&mut visual, 2);
            assert!(
                visual.debug_bounds("MENU_ITEM-Name").is_none(),
                "closing #{attempt} hides the menu"
            );
            assert_eq!(
                visual.update(|window, cx| window.focused(cx)),
                Some(start_focus),
                "after close #{attempt}, focus must return to where opening #{attempt} started"
            );
        }
    });
}

#[gpui::test]
fn escape_closes_only_the_open_menu_and_not_enclosing_context(cx: &mut TestAppContext) {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init_logger();
    let trace = AssetTrace::default();

    tracing::subscriber::with_default(trace, || {
        tracing::callsite::rebuild_interest_cache();
        cx.update(|cx| {
            cx.bind_keys([KeyBinding::new("escape", DismissHost, Some("Host"))]);
            yard_ui::init(cx);
        });
        let chosen = Rc::new(Cell::new(None));
        let host_dismissals = Rc::new(Cell::new(0));
        let host_dismissals_for_view = host_dismissals.clone();
        let origin_cell: Rc<std::cell::RefCell<Option<FocusHandle>>> = Rc::default();
        let origin_for_view = origin_cell.clone();
        let window = cx.open_window(size(px(600.), px(400.)), move |_, cx| {
            let origin = cx.focus_handle();
            *origin_for_view.borrow_mut() = Some(origin.clone());
            InteractiveDropdown {
                origin,
                selected: Some(0),
                options: vec!["Name".into(), "Date".into()],
                chosen,
                host_dismissals: host_dismissals_for_view,
                underlay_clicks: None,
            }
        });
        let origin_focus = origin_cell.borrow().clone().unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        step_frames(&mut visual, 2);

        visual.update(|window, cx| window.focus(&origin_focus, cx));
        step_frames(&mut visual, 1);

        let trigger = visual
            .debug_bounds("sort")
            .expect("dropdown trigger bounds");
        visual.simulate_click(trigger.center(), Modifiers::default());
        step_frames(&mut visual, 3);
        assert!(
            visual.debug_bounds("MENU_ITEM-Name").is_some(),
            "dropdown menu opens before Escape"
        );

        visual.simulate_keystrokes("escape");
        step_frames(&mut visual, 2);
        assert!(
            visual.debug_bounds("MENU_ITEM-Name").is_none(),
            "Escape closes the open dropdown menu"
        );
        assert_eq!(
            host_dismissals.get(),
            0,
            "Escape on an open dropdown menu must close only the menu, not the enclosing context"
        );

        visual.simulate_keystrokes("escape");
        step_frames(&mut visual, 1);
        assert_eq!(
            host_dismissals.get(),
            1,
            "once the menu is closed, Escape reaches the enclosing context"
        );
    });
}

#[gpui::test]
fn clicking_option_over_clickable_element_fires_only_the_option(cx: &mut TestAppContext) {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init_logger();
    let trace = AssetTrace::default();

    tracing::subscriber::with_default(trace, || {
        tracing::callsite::rebuild_interest_cache();
        cx.update(yard_ui::init);
        let chosen = Rc::new(Cell::new(None));
        let chosen_for_view = chosen.clone();
        let underlay_clicks = Rc::new(Cell::new(0));
        let underlay_for_view = underlay_clicks.clone();
        let window = cx.open_window(size(px(600.), px(400.)), move |_, cx| InteractiveDropdown {
            origin: cx.focus_handle(),
            selected: Some(0),
            options: vec!["Name".into(), "Date".into()],
            chosen: chosen_for_view,
            host_dismissals: Rc::new(Cell::new(0)),
            underlay_clicks: Some(underlay_for_view),
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        step_frames(&mut visual, 2);

        // Confirm the underlay is clickable when uncovered, then reset the counter.
        visual.simulate_click(point(px(500.), px(350.)), Modifiers::default());
        assert_eq!(underlay_clicks.get(), 1);
        underlay_clicks.set(0);

        let trigger = visual
            .debug_bounds("sort")
            .expect("dropdown trigger bounds");
        visual.simulate_click(trigger.center(), Modifiers::default());
        step_frames(&mut visual, 3);
        assert_eq!(underlay_clicks.get(), 0);

        let option = visual
            .debug_bounds("MENU_ITEM-Date")
            .expect("open dropdown 'Date' option bounds");
        let underlay = visual.debug_bounds("underlay").expect("underlay bounds");
        assert!(
            underlay.contains(&option.center()),
            "the open option overlaps the clickable element underneath"
        );

        visual.simulate_click(option.center(), Modifiers::default());
        step_frames(&mut visual, 2);

        assert_eq!(chosen.get(), Some(1), "clicking the option chooses it");
        assert_eq!(
            underlay_clicks.get(),
            0,
            "clicking an option over another clickable element must fire only the option"
        );
        assert!(visual.debug_bounds("MENU_ITEM-Date").is_none());
    });
}

fn draw_open_dropdown_with_assets(selected: Option<usize>) -> (Vec<String>, Vec<String>) {
    DIAGNOSTICS.lock().unwrap().clear();
    let trace = AssetTrace::default();

    tracing::subscriber::with_default(trace.clone(), || {
        tracing::callsite::rebuild_interest_cache();
        let mut headless = gpui::HeadlessAppContext::with_asset_source(
            Arc::new(gpui::NoopTextSystem),
            Arc::new(Assets),
        );
        headless.update(yard_ui::init);
        let asset_window = headless
            .open_window(size(px(600.), px(400.)), move |_, cx| {
                cx.new(|_| DropdownAssetView { selected })
            })
            .unwrap();

        headless
            .update_window(asset_window.into(), |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);

                window.focus_next(cx);
                window.refresh();
                window.draw(cx).clear(cx);

                let enter = Keystroke::parse("enter").unwrap();
                window.dispatch_event(
                    PlatformInput::KeyDown(KeyDownEvent {
                        keystroke: enter.clone(),
                        is_held: false,
                        prefer_character_input: false,
                    }),
                    cx,
                );
                window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke: enter }), cx);

                for _ in 0..3 {
                    window.simulate_next_frame(cx);
                    window.refresh();
                    window.draw(cx).clear(cx);
                }
            })
            .unwrap();
    });

    let loaded = trace.0.lock().unwrap().clone();
    let diagnostics = DIAGNOSTICS.lock().unwrap().clone();
    (loaded, diagnostics)
}

#[gpui::test]
fn open_menu_loads_bundled_trigger_and_checked_icons_without_warning(_cx: &mut TestAppContext) {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init_logger();

    let (unchecked_loaded, unchecked_diagnostics) = draw_open_dropdown_with_assets(None);
    assert!(
        unchecked_loaded
            .iter()
            .any(|path| path == "icons/chevron_down.svg"),
        "rendering the dropdown trigger must load the bundled chevron icon, got: {unchecked_loaded:?}"
    );
    assert!(
        !unchecked_loaded
            .iter()
            .any(|path| path == "icons/check.svg"),
        "an open menu with no selected option must not paint a check icon, got: {unchecked_loaded:?}"
    );
    assert!(
        unchecked_diagnostics.is_empty(),
        "rendering an unchecked open menu must log no missing-asset warning, got: {unchecked_diagnostics:#?}"
    );

    let (checked_loaded, checked_diagnostics) = draw_open_dropdown_with_assets(Some(0));
    assert!(
        checked_loaded
            .iter()
            .any(|path| path == "icons/chevron_down.svg"),
        "rendering the dropdown trigger must load the bundled chevron icon, got: {checked_loaded:?}"
    );
    assert!(
        checked_loaded.iter().any(|path| path == "icons/check.svg"),
        "rendering the open menu with a chosen option must load the bundled check icon, got: {checked_loaded:?}"
    );
    assert!(
        checked_diagnostics.is_empty(),
        "rendering the open menu with bundled assets must log no missing-asset warning, got: {checked_diagnostics:#?}"
    );
}
