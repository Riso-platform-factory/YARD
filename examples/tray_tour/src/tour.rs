use std::{cell::RefCell, io::Cursor, rc::Rc};

use gpui::{
    App, AppContext, Context, IntoElement, Menu, MenuItem, ParentElement, Render, Styled, Window,
};
use image::{ImageFormat, Rgba, RgbaImage};
use yard_shell::{Extension, NewWindow, Quit, Tray, TrayError, TrayIcon};
use yard_ui::{Button, Label, palette, v_flex};

gpui::actions!(tray_tour, [SayHello, Inner, Again]);

#[derive(Default)]
struct State {
    hello: usize,
    inner: usize,
    activations: usize,
    result: String,
}

pub fn init(cx: &mut App, ext: &Extension) {
    if std::env::var("TRAY_TOUR").as_deref() != Ok("1") {
        return;
    }

    let _group = tracing::info_span!("tray-tour-init").entered();
    let [red, green, blue] = if cfg!(target_os = "macos") {
        [0, 0, 0]
    } else {
        [69, 140, 225]
    };
    let icon = RgbaImage::from_fn(36, 36, |x, y| {
        let distance = x.abs_diff(18) + y.abs_diff(18);
        let alpha = if (6..=14).contains(&distance) { 255 } else { 0 };
        Rgba([red, green, blue, alpha])
    });
    let mut png = Cursor::new(Vec::new());
    icon.write_to(&mut png, ImageFormat::Png)
        .expect("encode the generated tray tour icon");
    let png: Rc<[u8]> = png.into_inner().into();

    let state = Rc::new(RefCell::new(State::default()));
    let hello = state.clone();
    ext.on_action::<SayHello>(cx, move |_, _, cx| {
        let mut state = hello.borrow_mut();
        state.hello += 1;
        state.result = "Say hello".into();
        cx.refresh_windows();
    });
    let inner = state.clone();
    ext.on_action::<Inner>(cx, move |_, _, cx| {
        let mut state = inner.borrow_mut();
        state.inner += 1;
        state.result = "Inner".into();
        cx.refresh_windows();
    });
    let again = state.clone();
    ext.on_action::<Again>(cx, move |_, _, cx| {
        again.borrow_mut().result = "Again".into();
        cx.refresh_windows();
    });
    let activate = state.clone();
    ext.on_tray_activate(cx, move |cx| {
        let mut state = activate.borrow_mut();
        state.activations += 1;
        state.result = "Activated".into();
        cx.refresh_windows();
    });

    let extension = ext.clone();
    ext.register(cx, "tray_tour.card", &[], move |_, _, _, cx| {
        cx.new(|_| Card {
            extension: extension.clone(),
            png: png.clone(),
            state: state.clone(),
        })
        .into()
    });
    ext.place_root(cx, "tray-tour-card", "tray_tour.card");
    tracing::debug!("placed the tray tour card");
}

#[derive(Clone, Copy)]
enum Command {
    Show,
    ChangeMenu,
    Remove,
    Invalid,
}

impl Command {
    fn run(self, card: &mut Card, cx: &mut Context<Card>) {
        let result: Result<&str, TrayError> = match self {
            Self::Show | Self::ChangeMenu => {
                let changed = matches!(self, Self::ChangeMenu);
                let mut menu = vec![
                    MenuItem::action("Say hello", SayHello),
                    MenuItem::separator(),
                    MenuItem::submenu(Menu::new("More").items([MenuItem::action("Inner", Inner)])),
                ];
                if changed {
                    menu.push(MenuItem::action("Again", Again));
                }
                menu.extend([
                    MenuItem::action("New Window", NewWindow),
                    MenuItem::action("Quit", Quit),
                ]);
                let tooltip = if changed {
                    "YARD tray tour updated"
                } else {
                    "YARD tray tour"
                };
                let tray = Tray::new(TrayIcon::png(&card.png).template(true))
                    .tooltip(tooltip)
                    .menu(menu);
                card.extension.set_tray(tray, cx).map(|()| {
                    if changed {
                        "Menu changed"
                    } else {
                        "Tray shown"
                    }
                })
            }
            Self::Remove => {
                card.extension.remove_tray(cx);
                Ok("Tray removed")
            }
            Self::Invalid => card
                .extension
                .set_tray(Tray::new(TrayIcon::png(b"invalid PNG")), cx)
                .map(|()| "Tray shown"),
        };
        card.state.borrow_mut().result = match result {
            Ok(message) => message.into(),
            Err(error) => format!("{error:?} · {error}"),
        };
        cx.refresh_windows();
    }
}

struct Card {
    extension: Extension,
    png: Rc<[u8]>,
    state: Rc<RefCell<State>>,
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = palette(cx);
        let view = cx.weak_entity();
        let buttons = [
            ("tray-tour-show", "Show tray", Command::Show),
            ("tray-tour-change", "Change menu", Command::ChangeMenu),
            ("tray-tour-remove", "Remove tray", Command::Remove),
            ("tray-tour-invalid", "Invalid icon", Command::Invalid),
        ]
        .map(|(id, label, command)| {
            let view = view.clone();
            Button::new(id, label).on_click(move |_, cx| {
                let _ = view.update(cx, |card, cx| command.run(card, cx));
            })
        });
        let state = self.state.borrow();
        v_flex()
            .size_full()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(colors.border)
            .children(buttons)
            .child(Label::new(format!(
                "Say hello: {} · Inner: {} · Activations: {}",
                state.hello, state.inner, state.activations
            )))
            .children((!state.result.is_empty()).then(|| Label::new(state.result.clone())))
    }
}
