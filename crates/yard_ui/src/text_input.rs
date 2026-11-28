//! A single-line text input.
//!
//! Adapted from `crates/gpui/examples/input.rs` of Zed at revision
//! ddb90e5e89d346f21847281727e1644b8bb2cc37, licensed Apache-2.0. That example
//! supplies the input handler and painting; editing and undo are ported from crew_ui.

mod editing;
mod failure;

use editing::{Edit, Editing};
use failure::Failure;
use gpui::{
    AccessibleAction, App, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, Element,
    ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, Font, GlobalElementId, Hsla, InspectorElementId, KeyBinding, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, Role, SharedString,
    Style, Subscription, Task, TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window,
    WrappedLine, accesskit::ActionData, div, fill, point, prelude::*, px, relative, size,
};
use std::{ops::Range, rc::Rc, time::Duration};
use theme::ActiveTheme;

const KEY_CONTEXT: &str = "TextInput";
const BLINK: Duration = Duration::from_millis(500);

mod actions {
    use gpui::actions;
    actions!(
        text_input,
        [
            Backspace,
            Delete,
            DeleteWordBackward,
            DeleteToLineStart,
            Left,
            Right,
            WordLeft,
            WordRight,
            LineStart,
            LineEnd,
            SelectLeft,
            SelectRight,
            SelectWordLeft,
            SelectWordRight,
            SelectLineStart,
            SelectLineEnd,
            SelectAll,
            Copy,
            Cut,
            Paste,
            Undo,
            Redo,
            Enter,
        ]
    );
}
use actions::*;

/// Binds the platform's single-line editing keys.
pub fn bind_text_input_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    let mut bindings = vec![
        (
            "backspace",
            KeyBinding::new("backspace", Backspace, context),
        ),
        ("delete", KeyBinding::new("delete", Delete, context)),
        ("left", KeyBinding::new("left", Left, context)),
        ("right", KeyBinding::new("right", Right, context)),
        ("home", KeyBinding::new("home", LineStart, context)),
        ("end", KeyBinding::new("end", LineEnd, context)),
        (
            "shift-left",
            KeyBinding::new("shift-left", SelectLeft, context),
        ),
        (
            "shift-right",
            KeyBinding::new("shift-right", SelectRight, context),
        ),
        (
            "shift-home",
            KeyBinding::new("shift-home", SelectLineStart, context),
        ),
        (
            "shift-end",
            KeyBinding::new("shift-end", SelectLineEnd, context),
        ),
        ("enter", KeyBinding::new("enter", Enter, context)),
    ];
    #[cfg(target_os = "macos")]
    bindings.extend([
        (
            "alt-backspace",
            KeyBinding::new("alt-backspace", DeleteWordBackward, context),
        ),
        (
            "cmd-backspace",
            KeyBinding::new("cmd-backspace", DeleteToLineStart, context),
        ),
        ("alt-left", KeyBinding::new("alt-left", WordLeft, context)),
        (
            "alt-right",
            KeyBinding::new("alt-right", WordRight, context),
        ),
        ("cmd-left", KeyBinding::new("cmd-left", LineStart, context)),
        ("cmd-right", KeyBinding::new("cmd-right", LineEnd, context)),
        ("ctrl-a", KeyBinding::new("ctrl-a", LineStart, context)),
        ("ctrl-e", KeyBinding::new("ctrl-e", LineEnd, context)),
        (
            "alt-shift-left",
            KeyBinding::new("alt-shift-left", SelectWordLeft, context),
        ),
        (
            "alt-shift-right",
            KeyBinding::new("alt-shift-right", SelectWordRight, context),
        ),
        (
            "cmd-shift-left",
            KeyBinding::new("cmd-shift-left", SelectLineStart, context),
        ),
        (
            "cmd-shift-right",
            KeyBinding::new("cmd-shift-right", SelectLineEnd, context),
        ),
        (
            "ctrl-shift-a",
            KeyBinding::new("ctrl-shift-a", SelectLineStart, context),
        ),
        (
            "ctrl-shift-e",
            KeyBinding::new("ctrl-shift-e", SelectLineEnd, context),
        ),
        ("cmd-a", KeyBinding::new("cmd-a", SelectAll, context)),
        ("cmd-c", KeyBinding::new("cmd-c", Copy, context)),
        ("cmd-x", KeyBinding::new("cmd-x", Cut, context)),
        ("cmd-v", KeyBinding::new("cmd-v", Paste, context)),
        ("cmd-z", KeyBinding::new("cmd-z", Undo, context)),
        ("cmd-shift-z", KeyBinding::new("cmd-shift-z", Redo, context)),
    ]);
    #[cfg(not(target_os = "macos"))]
    bindings.extend([
        (
            "ctrl-backspace",
            KeyBinding::new("ctrl-backspace", DeleteWordBackward, context),
        ),
        ("ctrl-left", KeyBinding::new("ctrl-left", WordLeft, context)),
        (
            "ctrl-right",
            KeyBinding::new("ctrl-right", WordRight, context),
        ),
        (
            "ctrl-shift-left",
            KeyBinding::new("ctrl-shift-left", SelectWordLeft, context),
        ),
        (
            "ctrl-shift-right",
            KeyBinding::new("ctrl-shift-right", SelectWordRight, context),
        ),
        ("ctrl-a", KeyBinding::new("ctrl-a", SelectAll, context)),
        ("ctrl-c", KeyBinding::new("ctrl-c", Copy, context)),
        ("ctrl-x", KeyBinding::new("ctrl-x", Cut, context)),
        ("ctrl-v", KeyBinding::new("ctrl-v", Paste, context)),
        ("ctrl-z", KeyBinding::new("ctrl-z", Undo, context)),
        (
            "ctrl-shift-z",
            KeyBinding::new("ctrl-shift-z", Redo, context),
        ),
        ("ctrl-y", KeyBinding::new("ctrl-y", Redo, context)),
    ]);
    let _group = tracing::info_span!("text-input-keys", count = bindings.len()).entered();
    for (keys, binding) in bindings {
        tracing::debug!(
            keys,
            action = binding.action().name(),
            "binding text input key"
        );
        cx.bind_keys([binding]);
    }
}

/// What a [`TextInput`] tells its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextInputEvent {
    /// The text changed.
    Changed,
    /// The user pressed Enter. The text is not cleared.
    Confirmed,
}

#[derive(Clone, Copy)]
enum Motion {
    Left,
    Right,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
}

/// The shaped single line, retained for hit testing and input methods.
struct Layout {
    lines: Vec<WrappedLine>,
    line_height: Pixels,
    bounds: Bounds<Pixels>,
    text_len: usize,
}
impl Layout {
    fn position(&self, offset: usize) -> Point<Pixels> {
        self.lines
            .first()
            .and_then(|line| line.position_for_index(offset.min(self.text_len), self.line_height))
            .unwrap_or_default()
    }
    fn offset(&self, position: Point<Pixels>) -> usize {
        self.lines.first().map_or(0, |line| {
            let index = match line.closest_index_for_position(
                point(position.x.max(Pixels::ZERO), Pixels::ZERO),
                self.line_height,
            ) {
                Ok(index) | Err(index) => index,
            };
            index.min(self.text_len)
        })
    }
}

/// A borderless single-line input with selection, clipboard, undo and IME composition.
pub struct TextInput {
    focus: FocusHandle,
    editing: Editing,
    label: Option<SharedString>,
    placeholder: SharedString,
    layout: Option<Rc<Layout>>,
    scroll: Pixels,
    selecting: bool,
    focused: bool,
    blink_on: bool,
    active: bool,
    failed: Failure,
    _blink: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<TextInputEvent> for TextInput {}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TextInput {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.on_focus(&focus, window, |this, _, cx| {
                this.focused = true;
                this.show_cursor(cx);
            }),
            cx.on_blur(&focus, window, |this, _, cx| {
                this.focused = false;
                this.selecting = false;
                cx.notify();
            }),
        ];
        let blink = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK).await;
                let alive = this.update(cx, |this, cx| {
                    if !this.focused {
                        return;
                    }
                    this.blink_on = this.active || !this.blink_on;
                    this.active = false;
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        Self {
            focus,
            editing: Editing::new(),
            label: None,
            placeholder: SharedString::default(),
            layout: None,
            scroll: Pixels::ZERO,
            selecting: false,
            focused: false,
            blink_on: true,
            active: false,
            failed: Failure::default(),
            _blink: blink,
            _subscriptions: subscriptions,
        }
    }

    /// Sets the name announced by assistive technology.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    fn accessible_name(&self) -> SharedString {
        self.label
            .clone()
            .unwrap_or_else(|| self.placeholder.clone())
    }

    /// The text shown, muted, while the input is empty.
    pub fn placeholder(mut self, text: impl Into<SharedString>) -> Self {
        self.placeholder = text.into();
        self
    }

    pub fn text(&self) -> &str {
        self.editing.text()
    }

    /// Replaces the text and forgets the undo history.
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = single_line(&text.into());
        let changed = self.text() != text;
        self.editing.set_text(text);
        self.scroll = Pixels::ZERO;
        if changed {
            self.changed(cx);
        } else {
            self.show_cursor(cx);
        }
    }

    /// Replaces the value as an edit and moves the caret to the end.
    fn set_value_from_assistive_technology(
        &mut self,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _group = tracing::info_span!("text-input-set-value", length = value.len()).entered();
        let value = single_line(&value);
        if value == self.text() {
            return;
        }

        self.edit(Some(0..self.text().len()), &value, Edit::Other, cx);
        window.notify_text_input_changed(&self.focus);
        tracing::debug!("set");
    }

    fn show_cursor(&mut self, cx: &mut Context<Self>) {
        self.blink_on = true;
        self.active = true;
        cx.notify();
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.show_cursor(cx);
        self.failed.changed();
        tracing::trace!(length = self.text().len(), "text changed");
        cx.emit(TextInputEvent::Changed);
    }

    fn edit(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        edit: Edit,
        cx: &mut Context<Self>,
    ) {
        if self.editing.replace(range, text, edit) {
            self.changed(cx);
        } else {
            self.show_cursor(cx);
        }
    }

    fn target(&mut self, motion: Motion) -> usize {
        let cursor = self.editing.cursor();
        match motion {
            Motion::Left => self.editing.previous_grapheme(cursor),
            Motion::Right => self.editing.next_grapheme(cursor),
            Motion::WordLeft => self.editing.previous_word(cursor),
            Motion::WordRight => self.editing.next_word(cursor),
            Motion::LineStart => 0,
            Motion::LineEnd => self.text().len(),
        }
    }

    fn go(&mut self, motion: Motion, select: bool, window: &mut Window, cx: &mut Context<Self>) {
        let selection = self.editing.selection();
        if select {
            let target = self.target(motion);
            self.editing.select_to(target);
        } else if !selection.is_empty() && matches!(motion, Motion::Left | Motion::Right) {
            // Leaving a selection sideways lands on its edge.
            self.editing.move_to(if matches!(motion, Motion::Left) {
                selection.start
            } else {
                selection.end
            });
        } else {
            let target = self.target(motion);
            self.editing.move_to(target);
        }
        window.notify_text_selection_changed(&self.focus);
        self.show_cursor(cx);
    }

    fn delete(&mut self, motion: Motion, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.selection().is_empty() {
            let target = self.target(motion);
            self.editing.select_to(target);
        }
        let selection = self.editing.selection();
        let start = self.editing.grapheme_boundary(selection.start);
        let end = if self.editing.grapheme_boundary(selection.end) == selection.end {
            selection.end
        } else {
            self.editing.next_grapheme(selection.end)
        };
        self.edit(Some(start..end), "", Edit::Other, cx);
        window.notify_text_input_changed(&self.focus);
    }

    fn copy(&mut self, cx: &mut Context<Self>) -> bool {
        let selection = self.editing.selection();
        if selection.is_empty() {
            return false;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(
            self.editing.text()[selection].to_owned(),
        ));
        true
    }

    fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        if self.editing.marked().is_some() {
            return;
        }
        tracing::debug!(length = self.text().len(), "text confirmed");
        cx.emit(TextInputEvent::Confirmed);
    }

    /// The offset under a point in the window.
    fn offset_at(&self, position: Point<Pixels>) -> usize {
        let Some(layout) = &self.layout else {
            return 0;
        };
        self.editing.grapheme_boundary(layout.offset(point(
            position.x - layout.bounds.left() + self.scroll,
            Pixels::ZERO,
        )))
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.selecting = true;
        let offset = self.offset_at(event.position);
        if event.modifiers.shift {
            self.editing.select_to(offset);
        } else {
            match event.click_count {
                2 => {
                    let word = self.editing.word_at(offset);
                    self.editing.select(word);
                }
                _ => self.editing.move_to(offset),
            }
        }
        window.notify_text_selection_changed(&self.focus);
        self.show_cursor(cx);
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            let offset = self.offset_at(event.position);
            self.editing.select_to(offset);
            window.notify_text_selection_changed(&self.focus);
            self.show_cursor(cx);
        }
    }

    fn mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.editing.range_from_utf16(&range_utf16);
        actual_range.replace(self.editing.range_to_utf16(&range));
        Some(self.editing.text()[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.editing.range_to_utf16(&self.editing.selection()),
            reversed: self.editing.reversed(),
        })
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.editing.range_from_utf16(&range_utf16);
        self.editing.select(range);
        window.notify_text_selection_changed(&self.focus);
        self.show_cursor(cx);
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.editing.offset_to_utf16(self.editing.text().len()))
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.editing
            .marked()
            .map(|range| self.editing.range_to_utf16(&range))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing.unmark();
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16.map(|range| self.editing.range_from_utf16(&range));
        // One typed character joins the typing before it in the undo history.
        let mut graphemes = unicode_segmentation::UnicodeSegmentation::graphemes(new_text, true);
        let typing = graphemes.next().is_some_and(|g| g != "\n") && graphemes.next().is_none();
        let edit = if typing { Edit::Typing } else { Edit::Other };
        self.edit(range, &single_line(new_text), edit, cx);
        window.notify_text_input_changed(&self.focus);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16.map(|range| self.editing.range_from_utf16(&range));
        let before = self.text().to_owned();
        self.editing
            .compose(range, &single_line(new_text), new_selected_range_utf16);
        window.notify_text_input_changed(&self.focus);
        if self.text() != before {
            self.changed(cx);
        } else {
            self.show_cursor(cx);
        }
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // The IME places its candidate window at the start of the range.
        let layout = self.layout.as_ref()?;
        let range = self.editing.range_from_utf16(&range_utf16);
        let start = layout.position(range.start);
        let end = layout.position(range.end);
        let width = if end.y == start.y {
            (end.x - start.x).max(px(1.))
        } else {
            px(1.)
        };
        Some(Bounds::new(
            point(bounds.left() + start.x - self.scroll, bounds.top()),
            size(width, layout.line_height),
        ))
    }

    fn selection_bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Bounds<Pixels>> {
        self.bounds_for_range(range_utf16, bounds, window, cx)
            .into_iter()
            .collect()
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        self.layout.as_ref()?;
        Some(self.editing.offset_to_utf16(self.offset_at(point)))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct Prepaint {
    layout: Rc<Layout>,
    /// The lines to paint: the text, or the placeholder while it is empty.
    painted: Option<Vec<WrappedLine>>,
    selection: Vec<PaintQuad>,
    cursor: Option<PaintQuad>,
    scroll: Pixels,
}

struct TextStyleNow {
    font: Font,
    font_size: Pixels,
    line_height: Pixels,
}

fn shape(
    text: &str,
    marked: Option<Range<usize>>,
    color: Hsla,
    style: &TextStyleNow,
    window: &Window,
    failed: &Failure,
) -> Vec<WrappedLine> {
    let run = |len: usize, underlined: bool| TextRun {
        len,
        font: style.font.clone(),
        color,
        background_color: None,
        underline: underlined.then_some(UnderlineStyle {
            color: Some(color),
            thickness: px(1.),
            wavy: false,
        }),
        strikethrough: None,
    };
    let runs = match marked {
        Some(marked) => vec![
            run(marked.start, false),
            run(marked.len(), true),
            run(text.len() - marked.end, false),
        ],
        None => vec![run(text.len(), false)],
    };
    window
        .text_system()
        .shape_text(SharedString::new(text), style.font_size, &runs, None, None)
        .map(|lines| lines.into_vec())
        .unwrap_or_else(|error| {
            failed.report("shape", error);
            Vec::new()
        })
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = TextStyleNow;
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // The text style is only known here, not when the layout is measured.
        let text_style = window.text_style();
        let style = TextStyleNow {
            font: text_style.font(),
            font_size: text_style.font_size.to_pixels(window.rem_size()),
            line_height: window.line_height(),
        };
        let mut layout_style = Style::default();
        layout_style.size.width = relative(1.).into();
        layout_style.size.height = style.line_height.into();
        let id = window.request_layout(layout_style, [], cx);
        (id, style)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        style: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let colors = cx.theme().colors();
        let player = cx.theme().players().local();
        let input = self.input.read(cx);
        let text = input.editing.text();
        let lines = shape(
            text,
            input.editing.marked(),
            colors.editor_foreground,
            style,
            window,
            &input.failed,
        );
        let placeholder = (text.is_empty() && !input.placeholder.is_empty()).then(|| {
            shape(
                &input.placeholder,
                None,
                colors.text_placeholder,
                style,
                window,
                &input.failed,
            )
        });
        let layout = Rc::new(Layout {
            lines,
            text_len: text.len(),
            line_height: style.line_height,
            bounds,
        });

        let cursor = layout.position(input.editing.cursor());
        let width = layout.position(text.len()).x;
        let max_scroll = (width + px(2.) - bounds.size.width).max(Pixels::ZERO);
        let mut scroll = input.scroll.clamp(Pixels::ZERO, max_scroll);
        scroll = scroll
            .min(cursor.x)
            .max(cursor.x + px(2.) - bounds.size.width)
            .clamp(Pixels::ZERO, max_scroll);
        let origin = point(bounds.left() - scroll, bounds.top());
        let selection = input.editing.selection();
        let mut selection_quads = Vec::new();
        if !selection.is_empty() {
            let start = layout.position(selection.start);
            let end = layout.position(selection.end);
            selection_quads.push(fill(
                Bounds::new(
                    point(origin.x + start.x, origin.y),
                    size(end.x - start.x, layout.line_height),
                ),
                player.selection,
            ));
        }
        let cursor_quad = (selection.is_empty() && input.focused && input.blink_on).then(|| {
            fill(
                Bounds::new(
                    point(origin.x + cursor.x, origin.y),
                    size(px(2.), layout.line_height),
                ),
                player.cursor,
            )
        });
        Prepaint {
            layout,
            painted: placeholder,
            selection: selection_quads,
            cursor: cursor_quad,
            scroll,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let input = self.input.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase == gpui::DispatchPhase::Bubble {
                input.update(cx, |input, cx| input.mouse_move(event, window, cx));
            }
        });
        let focus = self.input.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        let layout = prepaint.layout.clone();
        let origin = point(bounds.left() - prepaint.scroll, bounds.top());
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for quad in prepaint.selection.drain(..) {
                window.paint_quad(quad);
            }
            let placeholder = prepaint.painted.take();
            let lines = placeholder.as_ref().unwrap_or(&layout.lines);
            for line in lines {
                if let Err(error) = line.paint(
                    origin,
                    layout.line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                ) {
                    let input = self.input.read(cx);
                    input.failed.report("paint", error);
                }
            }
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        });
        let scroll = prepaint.scroll;
        self.input.update(cx, |input, _| {
            input.layout = Some(layout);
            input.scroll = scroll;
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rem = crate::ui_font_size(cx);
        let font = crate::ui_font(cx);
        let font_size = rem * 0.875;
        let line_height = 1.618;
        let go = |motion: Motion, select: bool| {
            move |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                this.go(motion, select, window, cx)
            }
        };
        macro_rules! on {
            ($element:expr, $($action:ty => $handler:expr),+ $(,)?) => {
                $element$(.on_action(cx.listener({
                    let handler = $handler;
                    move |this, _: &$action, window, cx| handler(this, window, cx)
                })))+
            };
        }
        let view = cx.weak_entity();
        let input = div()
            .id(ElementId::View(cx.entity_id()))
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .role(Role::TextInput)
            .aria_label(self.accessible_name())
            .when(!self.placeholder.is_empty(), |input| {
                input.aria_placeholder(self.placeholder.clone())
            })
            .aria_value(SharedString::from(self.text().to_owned()))
            .on_a11y_action(AccessibleAction::SetValue, move |data, window, cx| {
                if let Some(ActionData::Value(value)) = data {
                    let _ = view.update(cx, |input, cx| {
                        input.set_value_from_assistive_technology(value.to_string(), window, cx);
                    });
                }
            })
            .cursor(CursorStyle::IBeam)
            .w_full()
            .font(font)
            .text_size(font_size)
            .line_height((font_size * line_height).round())
            .on_action(cx.listener(Self::enter))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up));
        on!(input,
            Left => go(Motion::Left, false),
            Right => go(Motion::Right, false),
            WordLeft => go(Motion::WordLeft, false),
            WordRight => go(Motion::WordRight, false),
            LineStart => go(Motion::LineStart, false),
            LineEnd => go(Motion::LineEnd, false),
            SelectLeft => go(Motion::Left, true),
            SelectRight => go(Motion::Right, true),
            SelectWordLeft => go(Motion::WordLeft, true),
            SelectWordRight => go(Motion::WordRight, true),
            SelectLineStart => go(Motion::LineStart, true),
            SelectLineEnd => go(Motion::LineEnd, true),
            Backspace => |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                this.delete(Motion::Left, window, cx)
            },
            Delete => |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                this.delete(Motion::Right, window, cx)
            },
            DeleteWordBackward => |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                this.delete(Motion::WordLeft, window, cx)
            },
            DeleteToLineStart => |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                this.delete(Motion::LineStart, window, cx)
            },
            SelectAll => |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                let end = this.editing.text().len();
                this.editing.select(0..end);
                window.notify_text_selection_changed(&this.focus);
                cx.notify();
            },
            Copy => |this: &mut Self, _: &mut Window, cx: &mut Context<Self>| {
                if this.copy(cx) { tracing::debug!(length = this.editing.selection().len(), "copied"); }
            },
            Cut => |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                if this.copy(cx) {
                    tracing::debug!(length = this.editing.selection().len(), "cut");
                    this.edit(None, "", Edit::Other, cx);
                    window.notify_text_input_changed(&this.focus);
                }
            },
            Paste => |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    let text = single_line(&text);
                    tracing::debug!(length = text.len(), "pasted");
                    this.edit(None, &text, Edit::Other, cx);
                    window.notify_text_input_changed(&this.focus);
                }
            },
            Undo => |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                if this.editing.undo() {
                    window.notify_text_input_changed(&this.focus);
                    this.changed(cx);
                }
            },
            Redo => |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                if this.editing.redo() {
                    window.notify_text_input_changed(&this.focus);
                    this.changed(cx);
                }
            },
        )
        .child(TextElement { input: cx.entity() })
    }
}

fn single_line(text: &str) -> String {
    text.replace("\r\n", " ")
        .replace(['\r', '\n', '\u{0085}', '\u{2028}', '\u{2029}'], " ")
}

#[cfg(test)]
mod tests;
