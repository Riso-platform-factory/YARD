use gpui::{AppContext, IntoElement, ParentElement, Styled, TestAppContext};
use std::{cell::Cell, rc::Rc};

struct Probe(Rc<Cell<Option<gpui::Bounds<gpui::Pixels>>>>);

impl gpui::Render for Probe {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let bounds = self.0.clone();
        // Deliberately unsized: the surrounding layout must stretch this view.
        gpui::canvas(move |value, _, _| bounds.set(Some(value)), |_, _, _, _| {})
    }
}

struct Slot(riso_shell::Slots);

impl gpui::Render for Slot {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .size_full()
            .grid()
            .grid_cols(1)
            .grid_rows(1)
            .children(self.0.views("body"))
    }
}

#[gpui::test]
fn unsized_nodes_fill_root_split_tabs_and_component_slots(cx: &mut TestAppContext) {
    for component in [
        "tour.probe",
        "riso.row",
        "riso.column",
        "riso.tabs",
        "tour.slot",
    ] {
        let mut app = cx.new_app();
        app.update(riso_ui::init);
        let measured = Rc::new(Cell::new(None));
        app.update(|cx| {
            let ext = riso_shell::Extension::new(cx, "tour").unwrap();
            let measured = measured.clone();
            ext.register(cx, "tour.probe", &[], move |_, _, _, cx| {
                cx.new(|_| Probe(measured.clone())).into()
            });
            ext.register(cx, "tour.slot", &["body"], |_, slots, _, cx| {
                cx.new(|_| Slot(slots)).into()
            });
            ext.place_root(cx, "root", component);
            if component != "tour.probe" {
                let slot = if component == "tour.slot" {
                    "body"
                } else {
                    "children"
                };
                ext.place(cx, "probe", "tour.probe", "root", slot);
                if matches!(component, "riso.row" | "riso.column") {
                    ext.place(cx, "other", "tour.probe", "root", slot);
                }
            }
        });
        let window = app.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), riso_shell::new_shell)
                .unwrap()
        });
        for (width, height) in [(640., 480.), (800., 600.)] {
            app.simulate_window_resize(
                window.into(),
                gpui::size(gpui::px(width), gpui::px(height)),
            );
            gpui::AnyWindowHandle::from(window)
                .update(&mut app, |_, window, cx| {
                    window.refresh();
                    let _ = window.draw(cx);
                })
                .unwrap();
            let actual = measured.get().expect("probe rendered");
            let expected = match component {
                "riso.row" => gpui::Bounds::new(
                    gpui::point(gpui::px(width / 2.), gpui::px(0.)),
                    gpui::size(gpui::px(width / 2.), gpui::px(height)),
                ),
                "riso.column" => gpui::Bounds::new(
                    gpui::point(gpui::px(0.), gpui::px(height / 2.)),
                    gpui::size(gpui::px(width), gpui::px(height / 2.)),
                ),
                "riso.tabs" => {
                    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
                    let bar = visual.debug_bounds("TABBAR-root").unwrap();
                    gpui::Bounds::new(
                        gpui::point(gpui::px(0.), bar.bottom()),
                        gpui::size(gpui::px(width), gpui::px(height) - bar.bottom()),
                    )
                }
                _ => gpui::Bounds::new(
                    gpui::point(gpui::px(0.), gpui::px(0.)),
                    gpui::size(gpui::px(width), gpui::px(height)),
                ),
            };
            assert_eq!(actual, expected, "{component} at {width}x{height}");
        }
    }
}
