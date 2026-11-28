use gpui::{
    AnyElement, App, AppContext, Context, DismissEvent, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement, IntoElement, ParentElement, Render, Role,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder, px,
    rems,
};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    io::{self, Write},
    path::Path,
    rc::Rc,
};
use yard_settings::{DescribedSection, WriteOutcome, WriteRefusal};
use yard_shell::Extension;
use yard_ui::{
    Button, Dropdown, Label, ListItem, Modal, ModalHeader, ModalView, Section, Switch, TextColor,
    TextInput, TextInputEvent, h_flex, v_flex,
};

gpui::actions!(settings_editor, [Open]);

#[cfg(target_os = "macos")]
const OPEN_KEY: &str = "cmd-,";
#[cfg(not(target_os = "macos"))]
const OPEN_KEY: &str = "ctrl-,";

/// Registers the settings editor action and key bindings.
pub fn init(cx: &mut App, ext: &Extension) {
    ext.bind_keys(cx, &[(OPEN_KEY, Box::new(Open), "Shell")]);
    ext.on_action::<Open>(cx, {
        let ext = ext.clone();
        move |_, window, cx| {
            let editor_ext = ext.clone();
            // TODO(settings-ui): Open settings in a separate window. Provide extension access to opening and managing a settings window.
            ext.toggle_modal(window, cx, move |window, cx| {
                SettingsEditor::new(editor_ext, window, cx)
            });
        }
    });
}

mod fields;
use fields::{FieldControl, field_control};
pub use fields::{FieldKind, NumberError, field_kind, field_title, parse_number};

fn number_error_message(error: NumberError, kind: &FieldKind) -> String {
    match (error, kind) {
        (NumberError::NotAnInteger, _) => "Enter a whole number.".into(),
        (
            NumberError::BelowMinimum,
            FieldKind::Number {
                minimum: Some(bound),
                ..
            },
        ) => format!("Enter a number of at least {bound}."),
        (
            NumberError::AboveMaximum,
            FieldKind::Number {
                maximum: Some(bound),
                ..
            },
        ) => format!("Enter a number of at most {bound}."),
        (NumberError::BelowMinimum | NumberError::AboveMaximum, _) => {
            "Enter a number in the supported range.".into()
        }
        _ => "Enter a number.".into(),
    }
}

struct FieldInput {
    input: Entity<TextInput>,
    control: FieldControl,
    published: PublishedInput,
    _confirm: Subscription,
    _blur: Subscription,
}

#[derive(PartialEq)]
struct PublishedInput {
    text: String,
    is_null: bool,
}

/// The settings editor modal opened by [`Open`].
///
/// Element IDs and GPUI debug selectors use the same names.
/// Section selectors use `section:<section>`. Field controls use
/// `field:<section>.<field>`. Zed's context menu exposes its options by
/// their labels through `MENU_ITEM-<label>` debug selectors.
/// Reset buttons use `reset:<section>.<field>`.
pub struct SettingsEditor {
    ext: Extension,
    focus: FocusHandle,
    selected: Option<&'static str>,
    inputs: HashMap<Rc<str>, FieldInput>,
    errors: HashMap<Rc<str>, String>,
    _observe: Vec<Subscription>,
}

impl SettingsEditor {
    /// Creates the view for use with [`Extension::toggle_modal`].
    pub fn new(ext: Extension, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _group = tracing::info_span!("settings-editor-open").entered();
        let sections = yard_settings::described_sections(cx);
        let view = cx.weak_entity();
        let observe = yard_settings::observe_described_sections(cx, move |cx| {
            let _ = view.update(cx, |_, cx| cx.notify());
        });
        tracing::debug!(sections = sections.len(), "opened");
        Self {
            ext,
            focus: cx.focus_handle(),
            selected: sections.first().map(|section| section.key),
            inputs: HashMap::new(),
            errors: HashMap::new(),
            _observe: observe,
        }
    }

    fn commit(&mut self, section: &'static str, field: &str, cx: &mut Context<Self>) {
        let Some(input) = self
            .inputs
            .get(field)
            .filter(|_| self.selected == Some(section))
        else {
            return;
        };
        let text = input.input.read(cx).text();
        let clearing = input.control.nullable && text.is_empty() && !input.published.is_null;
        if text == input.published.text && !clearing {
            return;
        }
        let _group = tracing::info_span!("settings-editor-change", section, field).entered();
        let value = if clearing {
            Ok(None)
        } else {
            match &input.control.kind {
                FieldKind::Text => Ok(Some(Value::String(text.to_owned()))),
                kind => parse_number(text, kind).map(Some),
            }
        };
        match value {
            Ok(value) => {
                let outcome = save(section, field, value, cx);
                self.record_outcome(field, outcome, cx);
            }
            Err(reason) => {
                tracing::info!(?reason, "refused");
                let message = number_error_message(reason, &input.control.kind);
                self.show_error(field, message, cx);
            }
        }
    }

    fn show_error(&mut self, field: &str, message: String, cx: &mut Context<Self>) {
        if let Some(input) = self.inputs.get(field) {
            input
                .input
                .update(cx, |text, cx| text.set_text(&input.published.text, cx));
        }
        self.errors.insert(field.into(), message);
        cx.notify();
    }

    fn record_outcome(&mut self, field: &str, outcome: WriteOutcome, cx: &mut Context<Self>) {
        if let WriteOutcome::Refused(reason) = outcome {
            tracing::info!(?reason, "refused");
            let message = match reason {
                WriteRefusal::UnknownSection
                | WriteRefusal::UnknownField
                | WriteRefusal::TypeMismatch => "This setting is unavailable.",
                WriteRefusal::NonObjectSection => "This section in settings.json is not an object.",
                WriteRefusal::InvalidValue => "This value does not fit the setting.",
                WriteRefusal::UnreadableFile => "Could not read settings.json.",
                WriteRefusal::WriteFailed => "Could not save settings.json.",
                WriteRefusal::ConcurrentChange => "Settings changed on disk. Try again.",
            };
            self.show_error(field, message.into(), cx);
        } else {
            self.errors.remove(field);
            cx.notify();
        }
    }

    fn commit_focused(&mut self, window: &Window, cx: &mut Context<Self>) {
        if let Some(section) = self.selected
            && let Some(field) = self.inputs.iter().find_map(|(field, input)| {
                input
                    .input
                    .focus_handle(cx)
                    .is_focused(window)
                    .then(|| field.clone())
            })
        {
            self.commit(section, &field, cx);
        }
    }

    fn input(
        &mut self,
        section: &'static str,
        field: Rc<str>,
        control: FieldControl,
        published: PublishedInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TextInput> {
        if let Some(input) = self.inputs.get_mut(field.as_ref()) {
            if input.published != published {
                input
                    .input
                    .update(cx, |input, cx| input.set_text(&published.text, cx));
                input.published = published;
                self.errors.remove(field.as_ref());
            }
            return input.input.clone();
        }
        let input = cx.new(|cx| {
            let mut input = TextInput::new(window, cx).label(field_title(&field));
            input.set_text(&published.text, cx);
            input
        });
        let focus = input.focus_handle(cx);
        let confirm = cx.subscribe_in(&input, window, {
            let field = field.clone();
            move |this, _, event, _, cx| {
                if *event == TextInputEvent::Confirmed {
                    this.commit(section, &field, cx);
                }
            }
        });
        let blur = cx.on_blur(&focus, window, {
            let field = field.clone();
            move |this, _, cx| this.commit(section, &field, cx)
        });
        self.inputs.insert(
            field,
            FieldInput {
                input: input.clone(),
                control,
                published,
                _confirm: confirm,
                _blur: blur,
            },
        );
        input
    }

    fn render_field(
        &mut self,
        section: &DescribedSection,
        name: &str,
        schema: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = section.key;
        let title = field_title(name);
        let field: Rc<str> = name.into();
        let id = format!("field:{key}.{name}");
        let value = &section.value[name];
        let modified = *value != section.defaults[name];
        let FieldControl { kind, nullable } = field_control(&section.schema, schema);
        let view = cx.weak_entity();
        let change = {
            let field = field.clone();
            move |value, cx: &mut App| {
                let _group =
                    tracing::info_span!("settings-editor-change", section = key, field = %field)
                        .entered();
                let outcome = save(key, &field, value, cx);
                let _ = view.update(cx, |this, cx| this.record_outcome(&field, outcome, cx));
            }
        };
        let control = match kind {
            FieldKind::Switch => {
                Switch::new(id, title.clone(), value.as_bool().unwrap_or_default())
                    .on_toggle({
                        let change = change.clone();
                        move |on, _, cx| change(Some(Value::Bool(on)), cx)
                    })
                    .into_any_element()
            }
            FieldKind::Choice(options) => {
                let selected = value
                    .as_str()
                    .and_then(|value| options.iter().position(|option| option == value))
                    .or_else(|| (nullable && value.is_null()).then_some(options.len()));
                let labels = options
                    .iter()
                    .cloned()
                    .map(Into::into)
                    .chain(nullable.then(|| default_choice_label(&options).into()))
                    .collect();
                Dropdown::new(id, title.clone(), labels, selected)
                    .on_select({
                        let change = change.clone();
                        move |index, _, cx| {
                            if let Some(option) = options.get(index) {
                                change(Some(Value::String(option.clone())), cx);
                            } else if nullable && index == options.len() {
                                change(None, cx);
                            }
                        }
                    })
                    .into_any_element()
            }
            FieldKind::Text | FieldKind::Number { .. } => {
                let published = if let Some(text) = value.as_str() {
                    text.to_owned()
                } else if value.is_number() {
                    value.to_string()
                } else {
                    String::new()
                };
                let input = self.input(
                    key,
                    field.clone(),
                    FieldControl { kind, nullable },
                    PublishedInput {
                        text: published,
                        is_null: value.is_null(),
                    },
                    window,
                    cx,
                );
                div()
                    .id(id.clone())
                    .debug_selector(|| id)
                    .w_full()
                    .child(input)
                    .into_any_element()
            }
            FieldKind::FileOnly => {
                // TODO(settings-ui): Edit objects and arrays in the editor. Add schema-driven controls that validate and save structured values.
                let ext = self.ext.clone();
                let view = cx.weak_entity();
                div()
                    .debug_selector(|| id.clone())
                    .child(
                        Button::new(id, "Edit in settings.json")
                            .aria_label(format!("Edit {title} in settings.json"))
                            .on_click({
                                let field = field.clone();
                                move |_, cx| {
                                    let opened =
                                        yard_settings::settings_path(cx).is_some_and(|path| {
                                            ensure_settings_file(&path).is_ok()
                                                && ext.open_path(&path, cx).is_ok()
                                        });
                                    let _ = view.update(cx, |this, cx| {
                                        if opened {
                                            this.errors.remove(field.as_ref());
                                        } else {
                                            this.errors.insert(
                                                field.clone(),
                                                "Could not open settings.json.".into(),
                                            );
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .into_any_element()
            }
        };
        let reset = modified.then(|| {
            let id = format!("reset:{key}.{name}");
            div().debug_selector(|| id.clone()).child(
                Button::new(id, "Reset")
                    .aria_label(format!("Reset {title}"))
                    .on_click(move |_, cx| change(None, cx)),
            )
        });
        v_flex()
            .w_full()
            .gap_1()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(Label::new(title))
                    .when(modified, |row| {
                        row.child(Label::new("Modified").color(TextColor::Muted))
                    })
                    .children(reset),
            )
            .children(
                schema["description"]
                    .as_str()
                    .map(|help| Label::new(help.to_owned()).color(TextColor::Muted)),
            )
            .child(control)
            .children(
                self.errors
                    .get(name)
                    .cloned()
                    .map(|message| Label::new(message).color(TextColor::Error)),
            )
            .into_any_element()
    }
}

fn default_choice_label(options: &[String]) -> String {
    for label in ["Use default", "Use default setting"] {
        if !options.iter().any(|option| option == label) {
            return label.into();
        }
    }
    // An enum may contain both labels. Keep the reset entry distinct.
    (2..)
        .map(|index| format!("Use default setting ({index})"))
        .find(|label| !options.contains(label))
        .expect("finite option list")
}

fn save(section: &str, field: &str, value: Option<Value>, cx: &mut App) -> WriteOutcome {
    match value {
        Some(value) => yard_settings::write_value(cx, section, field, value),
        None => yard_settings::reset_value(cx, section, field),
    }
}

fn ensure_settings_file(path: &Path) -> io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => file.write_all(b"{}\n"),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}

impl EventEmitter<DismissEvent> for SettingsEditor {}

impl Focusable for SettingsEditor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ModalView for SettingsEditor {}

impl Render for SettingsEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // TODO(settings-ui): Search across sections. Add a query that filters described fields by title and help across all sections.
        let sections = yard_settings::described_sections(cx);
        let selected = sections
            .iter()
            .find(|section| Some(section.key) == self.selected);
        let fields = selected
            .and_then(|section| {
                section.schema["properties"]
                    .as_object()
                    .map(|fields| (section, fields))
            })
            .into_iter()
            .flat_map(|(section, fields)| {
                fields
                    .iter()
                    .map(move |(name, schema)| (section, name, schema))
            })
            .map(|(section, name, schema)| self.render_field(section, name, schema, window, cx))
            .collect::<Vec<_>>();

        let view = cx.weak_entity();
        let empty = sections.is_empty();
        let rows = sections.into_iter().map(|section| {
            let key = section.key;
            let id = format!("section:{key}");
            let selected = Some(key) == self.selected;
            let view = view.clone();
            div().debug_selector(|| id.clone()).child(
                ListItem::new(id)
                    .aria_role(Role::ListBoxOption)
                    .aria_label(section.title.clone())
                    .aria_selected(selected)
                    .toggle_state(selected)
                    .child(Label::new(section.title))
                    .on_click(move |window, cx| {
                        let _ = view.update(cx, |this, cx| {
                            if this.selected != Some(key) {
                                this.commit_focused(window, cx);
                                this.focus.focus(window, cx);
                                this.selected = Some(key);
                                this.inputs.clear();
                                this.errors.clear();
                                cx.notify();
                            }
                        });
                    }),
            )
        });
        let size = window.viewport_size();
        let width =
            rems((f32::from(size.width) - 32.).clamp(1., 800.) / f32::from(window.rem_size()));
        let height = (size.height - px(140.)).clamp(px(96.), px(600.));
        Modal::new("settings-editor")
            .width(width)
            .track_focus(&self.focus)
            .header(ModalHeader::new().headline("Settings").on_dismiss({
                let view = cx.weak_entity();
                move |window, cx| {
                    let _ = view.update(cx, |this, cx| {
                        this.commit_focused(window, cx);
                        cx.emit(DismissEvent);
                    });
                }
            }))
            .section(
                Section::new().child(
                    h_flex()
                        .w_full()
                        .h(height)
                        .items_start()
                        .gap_4()
                        .child(
                            v_flex()
                                .id("settings-sections")
                                .role(Role::ListBox)
                                .aria_label("Settings sections")
                                .w_1_4()
                                .h_full()
                                .overflow_y_scroll()
                                .children(rows),
                        )
                        .child(
                            v_flex()
                                .id("settings-fields")
                                .h_full()
                                .min_w_0()
                                .flex_1()
                                .overflow_y_scroll()
                                .gap_3()
                                .pr_2()
                                .when(empty, |panel| {
                                    panel.child(Label::new("No settings available."))
                                })
                                .children(fields),
                        ),
                ),
            )
    }
}
