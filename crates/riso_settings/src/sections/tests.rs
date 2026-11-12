use super::*;

#[derive(serde::Deserialize, PartialEq, Debug)]
struct First {
    greeting: String,
    size: u32,
    enabled: bool,
}

impl Default for First {
    fn default() -> Self {
        Self {
            greeting: "hello".into(),
            size: 14,
            enabled: true,
        }
    }
}

impl Settings for First {
    const KEY: &'static str = "first";
}

#[derive(serde::Deserialize, PartialEq, Debug)]
struct Second {
    label: String,
    limit: usize,
}

impl Default for Second {
    fn default() -> Self {
        Self {
            label: "panel".into(),
            limit: 3,
        }
    }
}

impl Settings for Second {
    const KEY: &'static str = "second";
}

#[derive(serde::Deserialize, PartialEq, Debug)]
struct SameKeyAsFirst {
    count: u32,
}

impl Default for SameKeyAsFirst {
    fn default() -> Self {
        Self { count: 99 }
    }
}

impl Settings for SameKeyAsFirst {
    const KEY: &'static str = First::KEY;
}

#[derive(serde::Deserialize, PartialEq, Debug)]
struct Counted {
    call: u32,
}

impl Default for Counted {
    fn default() -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
        Self {
            call: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }
}

impl Settings for Counted {
    const KEY: &'static str = "counted";
}

fn assert_get_unregistered_panics<T: Settings>(cx: &gpui::App) {
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = get::<T>(cx);
    }))
    .expect_err("get of unregistered section must panic");
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&'static str>().copied())
        .expect("panic payload must be a string");
    let expected = format!(
        "section {} is read before it is registered",
        std::any::type_name::<T>()
    );
    assert_eq!(message, expected);
}

#[gpui::test]
fn registered_section_reads_as_its_default(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        init(cx);
        register::<First>(cx);
    });
    cx.read(|cx| {
        let value = get::<First>(cx);
        let expected = First::default();
        assert_eq!(value.greeting, expected.greeting);
        assert_eq!(value.size, expected.size);
        assert_eq!(value.enabled, expected.enabled);
        assert_eq!(value, &expected);
    });
}

#[gpui::test]
fn two_sections_with_different_keys_are_read_independently(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        init(cx);
        register::<First>(cx);
        register::<Second>(cx);
    });
    cx.read(|cx| {
        let first = get::<First>(cx);
        let second = get::<Second>(cx);
        assert_eq!(first.greeting, "hello");
        assert_eq!(first.size, 14);
        assert!(first.enabled);
        assert_eq!(second.label, "panel");
        assert_eq!(second.limit, 3);
        assert_eq!(first, &First::default());
        assert_eq!(second, &Second::default());
    });
}

#[gpui::test]
fn registering_same_type_twice_leaves_first_value(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        init(cx);
        register::<Counted>(cx);
        register::<Counted>(cx);
    });
    cx.read(|cx| {
        let value = get::<Counted>(cx);
        assert_eq!(value.call, 1);
        assert_ne!(value, &Counted::default());
    });
}

#[gpui::test]
fn registering_another_type_with_same_key_leaves_first_value(cx: &mut gpui::TestAppContext) {
    assert_eq!(SameKeyAsFirst::default().count, 99);
    cx.update(|cx| {
        init(cx);
        register::<First>(cx);
        register::<SameKeyAsFirst>(cx);
    });
    cx.read(|cx| {
        assert_eq!(get::<First>(cx), &First::default());
    });
}

#[gpui::test]
fn registering_another_type_with_same_key_is_refused(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        init(cx);
        register::<First>(cx);
        register::<SameKeyAsFirst>(cx);
    });
    cx.read(|cx| {
        assert_get_unregistered_panics::<SameKeyAsFirst>(cx);
    });
}

#[gpui::test]
fn get_before_register_panics(cx: &mut gpui::TestAppContext) {
    cx.update(init);
    cx.read(|cx| {
        assert_get_unregistered_panics::<First>(cx);
    });
}

#[gpui::test]
#[should_panic(expected = "riso_settings::init runs before a section is registered")]
fn register_before_init_panics(cx: &mut gpui::TestAppContext) {
    cx.update(register::<First>);
}

#[gpui::test]
#[should_panic(expected = "riso_settings::init runs once")]
fn second_init_panics(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        init(cx);
        init(cx);
    });
}
