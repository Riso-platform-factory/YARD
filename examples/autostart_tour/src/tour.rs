use gpui::{App, AppContext, Context, IntoElement, ParentElement, Render, Styled, Window};
use std::{cell::RefCell, rc::Rc};
use yard_shell::{AutostartError, AutostartStatus, Extension};
use yard_ui::{Button, Label, palette, v_flex};

type Status = Result<AutostartStatus, AutostartError>;

struct State {
    status: Status,
    enabled_during_run: bool,
}

pub fn init(cx: &mut App, ext: &Extension) {
    if std::env::var("AUTOSTART_TOUR").as_deref() != Ok("1") {
        return;
    }
    let _group = tracing::info_span!("autostart-tour-init").entered();
    let state = Rc::new(RefCell::new(State {
        status: ext.launch_at_login(cx),
        enabled_during_run: false,
    }));
    let quit_state = state.clone();
    let extension = ext.clone();
    cx.on_app_quit(move |cx| {
        if quit_state.borrow().enabled_during_run {
            let _ = extension.set_launch_at_login(false, cx);
        }
        async {}
    })
    .detach();

    let extension = ext.clone();
    ext.register(cx, "autostart_tour.card", &[], move |_, _, _, cx| {
        cx.new(|_| Card {
            extension: extension.clone(),
            state: state.clone(),
        })
        .into()
    });
    ext.place_root(cx, "autostart-tour-card", "autostart_tour.card");
    tracing::debug!("placed the autostart tour card");
}

struct Card {
    extension: Extension,
    state: Rc<RefCell<State>>,
}

#[derive(Clone, Copy)]
enum Command {
    Enable,
    Disable,
    Refresh,
}

impl Card {
    fn run(&mut self, command: Command, cx: &mut Context<Self>) {
        let mut state = self.state.borrow_mut();
        state.status = match command {
            Command::Enable => self.extension.launch_at_login(cx).and_then(|before| {
                let status = self.extension.set_launch_at_login(true, cx)?;
                if before == AutostartStatus::Disabled
                    && matches!(
                        status,
                        AutostartStatus::Enabled | AutostartStatus::NeedsApproval
                    )
                {
                    state.enabled_during_run = true;
                }
                Ok(status)
            }),
            Command::Disable => self
                .extension
                .set_launch_at_login(false, cx)
                .inspect(|status| {
                    if *status == AutostartStatus::Disabled {
                        state.enabled_during_run = false;
                    }
                }),
            Command::Refresh => self.extension.launch_at_login(cx),
        };
        cx.refresh_windows();
    }
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = palette(cx);
        let view = cx.weak_entity();
        let buttons = [
            ("autostart-tour-enable", "Enable", Command::Enable),
            ("autostart-tour-disable", "Disable", Command::Disable),
            ("autostart-tour-refresh", "Refresh", Command::Refresh),
        ]
        .map(|(id, label, command)| {
            let view = view.clone();
            Button::new(id, label).on_click(move |_, cx| {
                let _ = view.update(cx, |card, cx| card.run(command, cx));
            })
        });
        let status = match self.state.borrow().status {
            Ok(status) => format!("Launch at login: {status:?}"),
            Err(error) => format!("Launch at login: {error:?} · {error}"),
        };
        v_flex()
            .size_full()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(colors.border)
            .child(Label::new(status))
            .children(buttons)
    }
}
