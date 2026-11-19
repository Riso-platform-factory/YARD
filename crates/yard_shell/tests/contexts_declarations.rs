use gpui::{AppContext, TestAppContext};
use std::sync::Mutex;
use yard_shell as shell;

static ERRORS: Mutex<Vec<String>> = Mutex::new(Vec::new());
struct Diagnostics;
impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_shell") && metadata.level() == log::Level::Error
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            ERRORS.lock().unwrap().push(record.args().to_string());
        }
    }
    fn flush(&self) {}
}
struct Model(u32);

// Alone in this binary because it installs the process-wide diagnostic logger.
#[gpui::test]
fn declarations_refuse_bugs_and_keep_first_models_with_window_fallback(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Error);
    cx.update(|cx| {
        yard_ui::init(cx);
        let ext = shell::Extension::new(cx, "edge").unwrap();
        // Declarations may precede registration.
        ext.provide::<Model>(cx, "edge.giver", |_, cx| cx.new(|_| Model(10)));
        ext.require_context::<Model>(cx, "edge.leaf");
        for component in ["edge.giver", "edge.leaf"] {
            ext.register(cx, component, &["body"], |_, _, _, cx| {
                cx.new(|_| gpui::Empty).into()
            });
        }
        let first = cx.new(|_| Model(1));
        ext.provide_window(cx, first);
        ext.place_root(cx, "root", "yard.row");
        ext.place(cx, "giver", "edge.giver", "root", "children");
        ext.place(cx, "inside", "edge.leaf", "giver", "body");
        ext.place(cx, "outside", "edge.leaf", "root", "children");
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap();
        assert!(ERRORS.lock().unwrap().is_empty());
        ext.provide::<Model>(cx, "bad component", |_, cx| cx.new(|_| Model(0)));
        ext.provide::<Model>(cx, "guest.giver", |_, cx| cx.new(|_| Model(0)));
        ext.provide::<Model>(cx, "edge.giver", |_, cx| cx.new(|_| Model(0)));
        ext.require_context::<Model>(cx, "bad component");
        ext.require_context::<Model>(cx, "guest.leaf");
        ext.require_context::<Model>(cx, "edge.leaf");
        let second = cx.new(|_| Model(2));
        ext.provide_window(cx, second);
        let errors = ERRORS.lock().unwrap();
        assert_eq!(errors.len(), 7, "{errors:?}");
        for (text, count) in [
            ("bad component", 2),
            ("guest", 2),
            ("edge.giver", 1),
            ("edge.leaf", 1),
            ("window", 1),
        ] {
            assert_eq!(
                errors.iter().filter(|line| line.contains(text)).count(),
                count,
                "{errors:?}"
            );
        }
        assert_eq!(
            shell::context::<Model>(cx, "inside").unwrap().read(cx).0,
            10
        );
        assert_eq!(
            shell::context::<Model>(cx, "outside").unwrap().read(cx).0,
            1
        );
        assert!(shell::context::<String>(cx, "inside").is_none());
    });
}
