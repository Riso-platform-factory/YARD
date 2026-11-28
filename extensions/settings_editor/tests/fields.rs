#![cfg(feature = "dev")]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use settings_editor::{FieldKind, NumberError, field_kind, field_title, parse_number};
use yard_settings::Settings;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Order {
    #[default]
    Name,
    Date,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
enum RenamedMode {
    #[default]
    #[serde(rename = "high-contrast")]
    HighContrast,
    #[serde(rename = "solarized_dark")]
    SolarizedDark,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
enum FilterRule {
    #[default]
    All,
    Limit(u32),
    Named {
        pattern: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct Advanced {
    flag: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct RecursiveNode {
    next: Option<Box<RecursiveNode>>,
}

/// Test section
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct Sample {
    /// Show the preview.
    preview: bool,
    /// Sort order.
    sort: Order,
    /// Items per page.
    #[schemars(range(min = 1, max = 100))]
    page_size: u32,
    /// Greeting.
    greeting: String,
    /// Nested.
    advanced: Advanced,
}

impl Default for Sample {
    fn default() -> Self {
        Self {
            preview: false,
            sort: Order::Name,
            page_size: 10,
            greeting: "hi".into(),
            advanced: Advanced::default(),
        }
    }
}

impl Settings for Sample {
    const KEY: &'static str = "sample";
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct ComplexFields {
    opt_text: Option<String>,
    opt_u32: Option<u32>,
    opt_bool: Option<bool>,
    opt_enum: Option<Order>,
    renamed: RenamedMode,
    data_enum: FilterRule,
    /// Documented referenced enum (`RemoveRefSiblings` wraps `$ref` in single-item `allOf`).
    documented_order: Order,
    recursive: RecursiveNode,
    plain_u32: u32,
    plain_i64: i64,
    plain_f32: f32,
}

fn root_schema_inlined<T: JsonSchema>() -> Value {
    let mut settings = schemars::generate::SchemaSettings::draft07();
    settings.inline_subschemas = true;
    serde_json::to_value(settings.into_generator().into_root_schema_for::<T>())
        .expect("serialize inlined schema")
}

fn root_schema_with_definitions<T: JsonSchema>() -> Value {
    let settings = schemars::generate::SchemaSettings::draft07();
    serde_json::to_value(settings.into_generator().into_root_schema_for::<T>())
        .expect("serialize schema with definitions")
}

#[test]
fn field_kind_maps_each_property_of_sample_schema() {
    let schema = root_schema_inlined::<Sample>();
    let props = schema["properties"]
        .as_object()
        .expect("Sample schema has properties");

    assert_eq!(field_kind(&schema, &props["preview"]), FieldKind::Switch);
    assert_eq!(
        field_kind(&schema, &props["sort"]),
        FieldKind::Choice(vec!["name".into(), "date".into()])
    );
    let page_size_kind = field_kind(&schema, &props["page_size"]);
    assert!(matches!(page_size_kind, FieldKind::Number { .. }));
    assert_eq!(parse_number("1", &page_size_kind), Ok(json!(1)));
    assert_eq!(parse_number("100", &page_size_kind), Ok(json!(100)));
    assert_eq!(
        parse_number("0", &page_size_kind),
        Err(NumberError::BelowMinimum)
    );
    assert_eq!(
        parse_number("101", &page_size_kind),
        Err(NumberError::AboveMaximum)
    );
    assert_eq!(field_kind(&schema, &props["greeting"]), FieldKind::Text);
    assert_eq!(field_kind(&schema, &props["advanced"]), FieldKind::FileOnly);
}

#[test]
fn field_kind_maps_nullable_fields_renamed_variants_and_data_enums() {
    // `inline_subschemas = true`: `Option<Order>` has `"type": ["string", "null"]` with `null` in `"enum"`.
    let inlined = root_schema_inlined::<ComplexFields>();
    let inlined_props = inlined["properties"].as_object().unwrap();

    assert_eq!(
        field_kind(&inlined, &inlined_props["opt_text"]),
        FieldKind::Text
    );
    let opt_u32 = field_kind(&inlined, &inlined_props["opt_u32"]);
    assert!(matches!(opt_u32, FieldKind::Number { .. }));
    assert_eq!(parse_number("0", &opt_u32), Ok(json!(0)));
    assert_eq!(
        parse_number("4294967296", &opt_u32),
        Err(NumberError::AboveMaximum)
    );
    assert_eq!(
        field_kind(&inlined, &inlined_props["opt_bool"]),
        FieldKind::Switch
    );
    assert_eq!(
        field_kind(&inlined, &inlined_props["opt_enum"]),
        FieldKind::Choice(vec!["name".into(), "date".into()])
    );
    assert_eq!(
        field_kind(&inlined, &inlined_props["renamed"]),
        FieldKind::Choice(vec!["high-contrast".into(), "solarized_dark".into()])
    );
    assert_eq!(
        field_kind(&inlined, &inlined_props["data_enum"]),
        FieldKind::FileOnly
    );

    // Default `SchemaSettings::draft07()`: `Option<Order>` uses `"anyOf": [{"$ref": ...}, {"type": "null"}]`.
    let with_defs = root_schema_with_definitions::<ComplexFields>();
    let defs_props = with_defs["properties"].as_object().unwrap();

    assert_eq!(
        field_kind(&with_defs, &defs_props["opt_enum"]),
        FieldKind::Choice(vec!["name".into(), "date".into()])
    );
    assert_eq!(
        field_kind(&with_defs, &defs_props["data_enum"]),
        FieldKind::FileOnly
    );

    // Explicit `anyOf` with `null` first and non-nullable union (`FileOnly`).
    let custom = json!({
        "type": "object",
        "properties": {
            "null_first": {
                "anyOf": [
                    { "type": "null" },
                    { "type": "string", "enum": ["alpha", "beta"] }
                ]
            },
            "two_non_null_types": {
                "anyOf": [
                    { "type": "string" },
                    { "type": "integer" }
                ]
            }
        }
    });
    assert_eq!(
        field_kind(&custom, &custom["properties"]["null_first"]),
        FieldKind::Choice(vec!["alpha".into(), "beta".into()])
    );
    assert_eq!(
        field_kind(&custom, &custom["properties"]["two_non_null_types"]),
        FieldKind::FileOnly
    );
}

#[test]
fn field_kind_resolves_local_refs_single_all_of_and_guards_cyclic_refs() {
    let with_defs = root_schema_with_definitions::<ComplexFields>();
    let props = with_defs["properties"].as_object().unwrap();

    // Direct `$ref` into `definitions` (`RenamedMode`).
    assert_eq!(
        field_kind(&with_defs, &props["renamed"]),
        FieldKind::Choice(vec!["high-contrast".into(), "solarized_dark".into()])
    );
    // Documented `$ref` wrapped in single-item `allOf`.
    assert_eq!(
        field_kind(&with_defs, &props["documented_order"]),
        FieldKind::Choice(vec!["name".into(), "date".into()])
    );
    // Recursive struct from `schemars`.
    assert_eq!(
        field_kind(&with_defs, &props["recursive"]),
        FieldKind::FileOnly
    );

    // Hand-constructed `$ref`, `allOf`, dangling `$ref`, and direct/mutual `$ref` cycles.
    let root = json!({
        "type": "object",
        "properties": {
            "via_ref": { "$ref": "#/definitions/Toggle" },
            "single_all_of": { "allOf": [{ "type": "string" }] },
            "multi_all_of": { "allOf": [{ "type": "string" }, { "minLength": 1 }] },
            "dangling": { "$ref": "#/definitions/Missing" },
            "self_cycle": { "$ref": "#/definitions/SelfLoop" },
            "mutual_cycle": { "$ref": "#/definitions/LoopA" }
        },
        "definitions": {
            "Toggle": { "type": "boolean" },
            "SelfLoop": { "$ref": "#/definitions/SelfLoop" },
            "LoopA": { "allOf": [{ "$ref": "#/definitions/LoopB" }] },
            "LoopB": {
                "anyOf": [
                    { "$ref": "#/definitions/LoopA" },
                    { "type": "null" }
                ]
            }
        }
    });
    let root_props = root["properties"].as_object().unwrap();

    assert_eq!(field_kind(&root, &root_props["via_ref"]), FieldKind::Switch);
    assert_eq!(
        field_kind(&root, &root_props["single_all_of"]),
        FieldKind::Text
    );
    assert_eq!(
        field_kind(&root, &root_props["multi_all_of"]),
        FieldKind::FileOnly
    );
    assert_eq!(
        field_kind(&root, &root_props["dangling"]),
        FieldKind::FileOnly
    );
    assert_eq!(
        field_kind(&root, &root_props["self_cycle"]),
        FieldKind::FileOnly
    );
    assert_eq!(
        field_kind(&root, &root_props["mutual_cycle"]),
        FieldKind::FileOnly
    );
}

#[test]
fn field_title_humanizes_field_names() {
    assert_eq!(field_title("page_size"), "Page size");
    assert_eq!(field_title("preview"), "Preview");
}

#[test]
fn parse_number_enforces_exact_integers_format_bounds_and_float_limits() {
    let schema = root_schema_inlined::<Sample>();
    let bounded = field_kind(&schema, &schema["properties"]["page_size"]);

    assert_eq!(parse_number("20", &bounded), Ok(json!(20)));
    assert_eq!(parse_number("0", &bounded), Err(NumberError::BelowMinimum));
    assert_eq!(
        parse_number("101", &bounded),
        Err(NumberError::AboveMaximum)
    );
    assert_eq!(
        parse_number("2.5", &bounded),
        Err(NumberError::NotAnInteger)
    );
    assert_eq!(parse_number("abc", &bounded), Err(NumberError::NotANumber));
    assert_eq!(parse_number(" 7 ", &bounded), Ok(json!(7)));

    // Decimal point or exponent is refused for an integer even when mathematically whole.
    assert!(matches!(
        parse_number("1.0", &bounded),
        Err(NumberError::NotAnInteger | NumberError::NotANumber)
    ));
    assert!(matches!(
        parse_number("1e1", &bounded),
        Err(NumberError::NotAnInteger | NumberError::NotANumber)
    ));

    let numeric_root = root_schema_inlined::<ComplexFields>();
    let numeric_props = numeric_root["properties"].as_object().unwrap();

    // `u32`: format `uint32` without an explicit `maximum` in `schemars`.
    let u32_schema = json!({ "type": "integer", "format": "uint32" });
    for kind in [
        field_kind(&numeric_root, &numeric_props["plain_u32"]),
        field_kind(&numeric_root, &u32_schema),
    ] {
        assert_eq!(parse_number("0", &kind), Ok(json!(0)));
        assert_eq!(
            parse_number("4294967295", &kind),
            Ok(json!(4_294_967_295_u64))
        );
        assert_eq!(
            parse_number("4294967296", &kind),
            Err(NumberError::AboveMaximum)
        );
        assert_eq!(parse_number("-1", &kind), Err(NumberError::BelowMinimum));
        assert!(matches!(
            parse_number("1.0", &kind),
            Err(NumberError::NotAnInteger | NumberError::NotANumber)
        ));
        assert!(matches!(
            parse_number("1e3", &kind),
            Err(NumberError::NotAnInteger | NumberError::NotANumber)
        ));
    }

    // `i64`: exact `i64::MAX` / `i64::MIN`, refusing one beyond either bound.
    let i64_kind = field_kind(&numeric_root, &numeric_props["plain_i64"]);
    assert_eq!(
        parse_number("9223372036854775807", &i64_kind),
        Ok(json!(9_223_372_036_854_775_807_i64))
    );
    assert_eq!(
        parse_number("9223372036854775808", &i64_kind),
        Err(NumberError::AboveMaximum)
    );
    assert_eq!(
        parse_number("-9223372036854775808", &i64_kind),
        Ok(json!(-9_223_372_036_854_775_808_i64))
    );
    assert_eq!(
        parse_number("-9223372036854775809", &i64_kind),
        Err(NumberError::BelowMinimum)
    );

    // `9007199254740993` (`2^53 + 1`) against a maximum of `9007199254740992` (`2^53`).
    let exact_max_root = json!({
        "type": "object",
        "properties": {
            "exact": {
                "type": "integer",
                "maximum": 9_007_199_254_740_992_i64
            }
        }
    });
    let exact_max_kind = field_kind(&exact_max_root, &exact_max_root["properties"]["exact"]);
    assert_eq!(
        parse_number("9007199254740992", &exact_max_kind),
        Ok(json!(9_007_199_254_740_992_i64))
    );
    assert_eq!(
        parse_number("9007199254740993", &exact_max_kind),
        Err(NumberError::AboveMaximum)
    );

    // `f32`: format `float` refuses `1e39`, `"inf"`, and `"NaN"`.
    let f32_kind = field_kind(&numeric_root, &numeric_props["plain_f32"]);
    assert_eq!(parse_number("1.5", &f32_kind), Ok(json!(1.5)));
    assert!(matches!(
        parse_number("1e39", &f32_kind),
        Err(NumberError::AboveMaximum | NumberError::NotANumber)
    ));
    assert!(matches!(
        parse_number("-1e39", &f32_kind),
        Err(NumberError::BelowMinimum | NumberError::NotANumber)
    ));
    assert_eq!(parse_number("inf", &f32_kind), Err(NumberError::NotANumber));
    assert_eq!(
        parse_number("-inf", &f32_kind),
        Err(NumberError::NotANumber)
    );
    assert_eq!(parse_number("NaN", &f32_kind), Err(NumberError::NotANumber));
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "A choice / ~")]
enum RenamedDefinitionChoice {
    #[default]
    Alpha,
    Beta,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
struct PercentEncodedRefSection {
    choice: RenamedDefinitionChoice,
    #[serde(default)]
    choice_with_default: RenamedDefinitionChoice,
}

#[test]
fn field_kind_percent_decodes_renamed_definition_ref() {
    let schema = root_schema_with_definitions::<PercentEncodedRefSection>();
    assert!(
        schema["definitions"].get("A choice / ~").is_some(),
        "expected renamed definition in Draft 07 schema: {schema:#}"
    );
    assert_eq!(
        schema["properties"]["choice"]["$ref"],
        json!("#/definitions/A%20choice%20~1%20~0")
    );
    assert_eq!(
        schema["properties"]["choice_with_default"]["allOf"][0]["$ref"],
        json!("#/definitions/A%20choice%20~1%20~0")
    );
    for prop in ["choice", "choice_with_default"] {
        assert_eq!(
            field_kind(&schema, &schema["properties"][prop]),
            FieldKind::Choice(vec!["alpha".into(), "beta".into()]),
            "expected Choice for {prop} in {schema:#}"
        );
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
struct Uint32Ref(u32);

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
struct AllOfBoundedSection {
    #[schemars(extend("maximum" = 100))]
    limit: Uint32Ref,
}

#[test]
fn all_of_wrapper_with_maximum_over_uint32_ref_enforces_both_bounds() {
    let generated = root_schema_with_definitions::<AllOfBoundedSection>();
    assert_eq!(generated["properties"]["limit"]["maximum"], json!(100));
    assert_eq!(
        generated["properties"]["limit"]["allOf"],
        json!([{ "$ref": "#/definitions/Uint32Ref" }])
    );
    let explicit = json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {
            "limit": {
                "allOf": [{ "$ref": "#/definitions/Uint32" }],
                "maximum": 100
            }
        },
        "definitions": {
            "Uint32": {
                "type": "integer",
                "format": "uint32"
            }
        }
    });

    for schema in [&generated, &explicit] {
        let kind = field_kind(schema, &schema["properties"]["limit"]);
        assert!(matches!(kind, FieldKind::Number { integer: true, .. }));
        assert_eq!(parse_number("100", &kind), Ok(json!(100)));
        assert_eq!(
            parse_number("101", &kind),
            Err(NumberError::AboveMaximum),
            "allOf wrapper maximum: 100 must refuse 101 for {schema:#}"
        );
        assert_eq!(parse_number("0", &kind), Ok(json!(0)));
        assert_eq!(parse_number("-1", &kind), Err(NumberError::BelowMinimum));
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ChoiceWithDefault {
    #[default]
    Default,
    Custom,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct NullableChoiceSection {
    mode: Option<ChoiceWithDefault>,
    keep: bool,
}

impl Settings for NullableChoiceSection {
    const KEY: &'static str = "nullable_choice";
}

struct RecordingTextSystem {
    inner: gpui::NoopTextSystem,
    lines: std::sync::Mutex<Vec<String>>,
}

impl RecordingTextSystem {
    fn new() -> Self {
        Self {
            inner: gpui::NoopTextSystem::new(),
            lines: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl gpui::PlatformTextSystem for RecordingTextSystem {
    fn add_fonts(&self, fonts: Vec<std::borrow::Cow<'static, [u8]>>) -> gpui::Result<()> {
        self.inner.add_fonts(fonts)
    }

    fn all_font_names(&self) -> Vec<String> {
        self.inner.all_font_names()
    }

    fn font_id(&self, descriptor: &gpui::Font) -> gpui::Result<gpui::FontId> {
        self.inner.font_id(descriptor)
    }

    fn font_metrics(&self, font_id: gpui::FontId) -> gpui::FontMetrics {
        self.inner.font_metrics(font_id)
    }

    fn typographic_bounds(
        &self,
        font_id: gpui::FontId,
        glyph_id: gpui::GlyphId,
    ) -> gpui::Result<gpui::Bounds<f32>> {
        self.inner.typographic_bounds(font_id, glyph_id)
    }

    fn advance(
        &self,
        font_id: gpui::FontId,
        glyph_id: gpui::GlyphId,
    ) -> gpui::Result<gpui::Size<f32>> {
        self.inner.advance(font_id, glyph_id)
    }

    fn glyph_for_char(&self, font_id: gpui::FontId, ch: char) -> Option<gpui::GlyphId> {
        self.inner.glyph_for_char(font_id, ch)
    }

    fn glyph_raster_bounds(
        &self,
        params: &gpui::RenderGlyphParams,
    ) -> gpui::Result<gpui::Bounds<gpui::DevicePixels>> {
        self.inner.glyph_raster_bounds(params)
    }

    fn rasterize_glyph(
        &self,
        params: &gpui::RenderGlyphParams,
        raster_bounds: gpui::Bounds<gpui::DevicePixels>,
    ) -> gpui::Result<(gpui::Size<gpui::DevicePixels>, Vec<u8>)> {
        self.inner.rasterize_glyph(params, raster_bounds)
    }

    fn layout_line(
        &self,
        text: &str,
        font_size: gpui::Pixels,
        runs: &[gpui::FontRun],
    ) -> gpui::LineLayout {
        self.lines.lock().unwrap().push(text.to_owned());
        self.inner.layout_line(text, font_size, runs)
    }

    fn recommended_rendering_mode(
        &self,
        font_id: gpui::FontId,
        font_size: gpui::Pixels,
    ) -> gpui::TextRenderingMode {
        self.inner.recommended_rendering_mode(font_id, font_size)
    }
}

struct EmptyCard;

impl gpui::Render for EmptyCard {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::Styled;
        gpui::div().size_full()
    }
}

#[gpui::test]
fn nullable_choice_reset_option_is_named_use_default_and_distinct_from_default_variant(
    cx: &mut gpui::TestAppContext,
) {
    use gpui::{AppContext, VisualTestContext};
    use std::{
        fs,
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
        time::{SystemTime, UNIX_EPOCH},
    };
    use yard_shell::Extension;

    let inlined = root_schema_inlined::<NullableChoiceSection>();
    assert_eq!(
        field_kind(&inlined, &inlined["properties"]["mode"]),
        FieldKind::Choice(vec!["default".into(), "custom".into()])
    );

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let dir = std::env::temp_dir().join(format!(
        "yard-settings-fields-{}-{}-{nanos:x}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed),
    ));
    fs::create_dir_all(&dir).unwrap();
    let settings_path = dir.join("settings.json");
    fs::write(&settings_path, r#"{"nullable_choice": {"keep": true}}"#).unwrap();

    let text_system = Arc::new(RecordingTextSystem::new());
    let mut app = gpui::TestAppContext::build_with_text_system(
        cx.dispatcher.clone(),
        None,
        text_system.clone(),
    );

    app.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, settings_path.clone());
        yard_ui::init(cx);
        yard_shell::bind_default_keys(cx);
        yard_ui::bind_text_input_keys(cx);

        let editor = Extension::new(cx, "settings_editor").unwrap();
        settings_editor::init(cx, &editor);

        let ext = Extension::new(cx, "nullable_choice").unwrap();
        ext.register_editable_settings::<NullableChoiceSection>(cx);
        ext.register(cx, "nullable_choice.card", &[], |_, _, _, cx| {
            cx.new(|_| EmptyCard).into()
        });
        ext.place_root(cx, "root", "nullable_choice.card");
    });

    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(gpui::px(1000.), gpui::px(800.)));
    let draw = |app: &mut gpui::TestAppContext, frames: usize| {
        for _ in 0..frames {
            gpui::AnyWindowHandle::from(window)
                .update(app, |_, window, cx| {
                    window.simulate_next_frame(cx);
                    window.refresh();
                    let _ = window.draw(cx);
                })
                .unwrap();
            app.run_until_parked();
        }
    };
    draw(&mut app, 1);

    let mut visual = VisualTestContext::from_window(window.into(), &app);
    visual.dispatch_action(settings_editor::Open);
    draw(&mut app, 2);

    let section_bounds = visual
        .debug_bounds("section:nullable_choice")
        .expect("section:nullable_choice is drawn");
    visual.simulate_click(section_bounds.center(), gpui::Modifiers::default());
    draw(&mut app, 1);

    let trigger_bounds = visual
        .debug_bounds("field:nullable_choice.mode")
        .expect("field:nullable_choice.mode is drawn");
    visual.simulate_click(trigger_bounds.center(), gpui::Modifiers::default());
    draw(&mut app, 3);

    assert!(visual.debug_bounds("MENU_ITEM-default").is_some());
    assert!(visual.debug_bounds("MENU_ITEM-custom").is_some());
    assert!(visual.debug_bounds("MENU_ITEM-Use default").is_some());
    assert!(visual.debug_bounds("MENU_ITEM-Default").is_none());

    // Choose option 0 (`"default"` variant): writes `"default"`, distinct from resetting to `None`.
    let opt0 = visual.debug_bounds("MENU_ITEM-default").unwrap();
    visual.simulate_click(opt0.center(), gpui::Modifiers::default());
    draw(&mut app, 3);

    let after_variant: Value = serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
    assert_eq!(
        after_variant,
        json!({"nullable_choice": {"keep": true, "mode": "default"}})
    );
    app.read(|cx| {
        assert_eq!(
            yard_settings::get::<NullableChoiceSection>(cx).mode,
            Some(ChoiceWithDefault::Default)
        );
    });
    assert!(
        visual.debug_bounds("reset:nullable_choice.mode").is_some(),
        "choosing the 'default' variant on a None field marks it modified"
    );

    // Choose option 2 ("Use default"): resets `mode` back to `None`.
    let trigger_bounds = visual
        .debug_bounds("field:nullable_choice.mode")
        .expect("field:nullable_choice.mode is drawn");
    visual.simulate_click(trigger_bounds.center(), gpui::Modifiers::default());
    draw(&mut app, 3);
    let opt2 = visual.debug_bounds("MENU_ITEM-Use default").unwrap();
    visual.simulate_click(opt2.center(), gpui::Modifiers::default());
    draw(&mut app, 3);

    let after_reset: Value = serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
    assert_eq!(after_reset, json!({"nullable_choice": {"keep": true}}));
    app.read(|cx| {
        assert_eq!(yard_settings::get::<NullableChoiceSection>(cx).mode, None);
    });
    assert!(visual.debug_bounds("reset:nullable_choice.mode").is_none());

    let shaped = text_system.lines.lock().unwrap().clone();
    let _ = fs::remove_dir_all(&dir);
    assert!(
        shaped.iter().any(|line| line == "default"),
        "expected 'default' variant in shaped labels, got: {shaped:?}"
    );
    assert!(
        shaped.iter().any(|line| line == "Use default"),
        "expected nullable choice reset option to be named 'Use default', got: {shaped:?}"
    );
    assert!(
        !shaped.iter().any(|line| line == "Default"),
        "nullable choice reset option must be 'Use default', not 'Default': {shaped:?}"
    );
}
