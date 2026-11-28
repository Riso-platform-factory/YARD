mod matching;
#[cfg(test)]
mod tests;

pub use matching::humanize_action_name;

use gpui::{
    Action, App, AppContext, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, ParentElement, Render, Role, ScrollStrategy, SharedString,
    StatefulInteractiveElement, Styled, Subscription, UniformListScrollHandle, Window, div, rems,
};
use matching::rank_candidates;
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};
use yard_shell::Extension;
use yard_ui::ModalSurface;
use yard_ui::{
    Divider, KeyHint, Label, List, ListItem, ListItemSpacing, ModalView, ScrollList, TextInput,
    TextInputEvent, h_flex,
};

gpui::actions!(command_palette, [Toggle]);

mod actions {
    gpui::actions!(command_palette, [SelectNext, SelectPrevious]);
}

const KEY_CONTEXT: &str = "CommandPalette";

#[cfg(target_os = "macos")]
const TOGGLE_KEY: &str = "cmd-shift-p";
#[cfg(not(target_os = "macos"))]
const TOGGLE_KEY: &str = "ctrl-shift-p";

/// The extension's init, listed by a composition root.
pub fn init(cx: &mut App, ext: &Extension) {
    ext.bind_keys(
        cx,
        &[
            (TOGGLE_KEY, Box::new(Toggle), "Shell"),
            ("up", Box::new(actions::SelectPrevious), KEY_CONTEXT),
            ("down", Box::new(actions::SelectNext), KEY_CONTEXT),
        ],
    );
    let modal_ext = ext.clone();
    ext.on_action::<Toggle>(cx, move |_, window, cx| {
        let palette_ext = modal_ext.clone();
        modal_ext.toggle_modal(window, cx, move |window, cx| {
            CommandPalette::new(palette_ext, window, cx)
        });
    });
}

struct CommandEntry {
    action: Box<dyn Action>,
    readable: SharedString,
}

fn is_excluded_action(name: &str) -> bool {
    name == Toggle::name_for_type()
        || name == "NoAction"
        || name == "Unbind"
        || name.ends_with("::NoAction")
        || name.ends_with("::Unbind")
}

fn collect_commands(window: &Window, cx: &App) -> Vec<CommandEntry> {
    let mut seen = BTreeSet::new();
    let mut commands = Vec::new();
    for action in window.available_actions(cx) {
        let name = action.name();
        if is_excluded_action(name) || !seen.insert(name.to_owned()) {
            continue;
        }
        let readable = SharedString::from(humanize_action_name(name));
        commands.push(CommandEntry { action, readable });
    }
    commands.sort_by(|left, right| {
        left.readable
            .as_ref()
            .cmp(right.readable.as_ref())
            .then_with(|| left.action.name().cmp(right.action.name()))
    });
    commands
}

struct CommandPalette {
    ext: Extension,
    previous_focus: Option<FocusHandle>,
    input: Entity<TextInput>,
    commands: Vec<CommandEntry>,
    matches: Vec<usize>,
    selected: usize,
    scroll_handle: UniformListScrollHandle,
    dispatched: bool,
    _subscription: Subscription,
}

impl CommandPalette {
    fn new(ext: Extension, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let previous_focus = window.focused(cx);
        let commands = collect_commands(window, cx);
        tracing::debug!(count = commands.len(), "commands listed");
        let matches: Vec<usize> = (0..commands.len()).collect();
        let input = cx.new(|cx| {
            TextInput::new(window, cx)
                .label("Command")
                .placeholder("Execute a command...")
        });
        let subscription =
            cx.subscribe_in(
                &input,
                window,
                |this, input, event, window, cx| match event {
                    TextInputEvent::Changed => {
                        let query = input.read(cx).text().to_owned();
                        this.update_matches(&query, cx);
                    }
                    TextInputEvent::Confirmed => {
                        this.confirm(window, cx);
                    }
                },
            );
        Self {
            ext,
            previous_focus,
            input,
            commands,
            matches,
            selected: 0,
            scroll_handle: UniformListScrollHandle::new(),
            dispatched: false,
            _subscription: subscription,
        }
    }

    fn update_matches(&mut self, query: &str, cx: &mut Context<Self>) {
        let names: Vec<&str> = self
            .commands
            .iter()
            .map(|entry| entry.readable.as_ref())
            .collect();
        self.matches = rank_candidates(query, &names);
        self.selected = 0;
        self.scroll_handle
            .scroll_to_item(0, ScrollStrategy::Nearest);
        tracing::trace!(
            query_length = query.len(),
            count = self.matches.len(),
            "commands matched",
        );
        cx.notify();
    }

    fn select_next(&mut self, _: &actions::SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        if self.matches.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.matches.len();
        self.scroll_handle
            .scroll_to_item(self.selected, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn select_previous(
        &mut self,
        _: &actions::SelectPrevious,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.matches.is_empty() {
            return;
        }
        self.selected = self
            .selected
            .checked_sub(1)
            .unwrap_or(self.matches.len() - 1);
        self.scroll_handle
            .scroll_to_item(self.selected, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.matches.is_empty() {
            return;
        }
        self.run_match(self.selected, window, cx);
    }

    fn run_match(&mut self, row_index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.dispatched {
            return;
        }
        let Some(&command_index) = self.matches.get(row_index) else {
            return;
        };
        let action = self.commands[command_index].action.boxed_clone();
        self.dispatched = true;
        let previous_focus = self.previous_focus.clone();
        self.ext.hide_modal(window, cx);
        if let Some(focus) = &previous_focus
            && !focus.contains_focused(window, cx)
        {
            window.focus(focus, cx);
        }
        window.defer(cx, move |window, cx| {
            if let Some(focus) = &previous_focus
                && !focus.contains_focused(window, cx)
            {
                window.focus(focus, cx);
            }
            let entered = Rc::new(RefCell::new(None));
            let entered_in = entered.clone();
            let action_name = SharedString::from(action.name());
            cx.defer(move |_| {
                *entered_in.borrow_mut() = Some(
                    tracing::info_span!("command-run", action = action_name.as_ref()).entered(),
                );
            });
            window.dispatch_action(action, cx);
            cx.defer(move |_| {
                entered.borrow_mut().take();
            });
        });
    }
}

impl EventEmitter<DismissEvent> for CommandPalette {}

impl Focusable for CommandPalette {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl ModalView for CommandPalette {}

fn row_content(readable: SharedString, hint: KeyHint) -> gpui::Div {
    h_flex()
        .w_full()
        .py_px()
        .justify_between()
        .gap_2()
        .child(div().min_w_0().truncate().child(Label::new(readable)))
        .child(h_flex().flex_shrink_0().gap_2().child(hint))
}

impl Render for CommandPalette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.matches.len();
        let body = if count == 0 {
            List::new()
                .id("command-palette-empty")
                .empty_message("No matches")
                .into_any_element()
        } else {
            // TODO(accessibility): Announce the highlighted command while typing.
            // Restructure the palette so the list is a descendant of the focused element.
            let selected = self.selected;
            let previous_focus = self.previous_focus.clone();
            let rows_data: Vec<(usize, SharedString, Box<dyn Action>)> = self
                .matches
                .iter()
                .map(|&command_index| {
                    let entry = &self.commands[command_index];
                    (
                        command_index,
                        entry.readable.clone(),
                        entry.action.boxed_clone(),
                    )
                })
                .collect();
            let palette = cx.weak_entity();
            let rem = window.rem_size();
            let max_height = (window.viewport_size().height - rem * 10.).max(gpui::px(0.)) * 0.95;
            let list_height = (rem * 24.).min(max_height);
            let (_, readable, action) = &rows_data[0];
            let hint = match &previous_focus {
                Some(focus) => KeyHint::for_action_in(action.as_ref(), focus, cx),
                None => KeyHint::for_action(action.as_ref(), cx),
            };
            let row_height = ListItem::new("palette-row-measure")
                .inset(true)
                .spacing(ListItemSpacing::Sparse)
                .child(row_content(readable.clone(), hint))
                .into_any_element()
                .layout_as_root(
                    gpui::size(
                        gpui::AvailableSpace::Definite(rem * 38.),
                        gpui::AvailableSpace::MinContent,
                    ),
                    window,
                    cx,
                )
                .height;
            let height =
                (row_height * count as f32).min((list_height - rem * 0.5).max(gpui::px(0.)));
            div()
                .id("command-palette-results")
                .role(Role::ListBox)
                .w_full()
                .py_1()
                .child(
                    ScrollList::new(
                        "command-palette-list",
                        count,
                        &self.scroll_handle,
                        move |range, _, cx| {
                            range
                                .map(|index| {
                                    let (command_index, readable, action) = &rows_data[index];
                                    let hint = match &previous_focus {
                                        Some(focus) => {
                                            KeyHint::for_action_in(action.as_ref(), focus, cx)
                                        }
                                        None => KeyHint::for_action(action.as_ref(), cx),
                                    };
                                    let palette = palette.clone();
                                    div()
                                        .w_full()
                                        .debug_selector(move || format!("PALETTE-ROW-{index}"))
                                        .child(
                                            ListItem::new(("palette-row", *command_index))
                                                .aria_role(Role::ListBoxOption)
                                                .aria_label(readable.clone())
                                                .aria_selected(index == selected)
                                                .inset(true)
                                                .spacing(ListItemSpacing::Sparse)
                                                .on_click(move |window, cx| {
                                                    let _ = palette.update(cx, |this, cx| {
                                                        this.run_match(index, window, cx);
                                                    });
                                                })
                                                .child(row_content(readable.clone(), hint)),
                                        )
                                        .into_any_element()
                                })
                                .collect()
                        },
                    )
                    .w_full()
                    .h(height),
                )
                .into_any_element()
        };

        div()
            .id("command-palette")
            .role(Role::Dialog)
            .aria_label("Command palette")
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .child(
                ModalSurface::new()
                    .w(rems(38.))
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .h_9()
                            .px_2p5()
                            .flex_none()
                            .overflow_hidden()
                            .child(div().flex_1().child(self.input.clone())),
                    )
                    .child(Divider::horizontal())
                    .child(body),
            )
    }
}
