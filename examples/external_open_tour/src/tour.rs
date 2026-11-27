use gpui::{
    App, AppContext, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window,
};
use std::{fs, io, path::PathBuf};
use yard_shell::{Extension, ExternalOpenError};
use yard_ui::{Button, Label, palette, v_flex};

pub fn init(cx: &mut App, ext: &Extension, scheme: &str) {
    if std::env::var("EXTERNAL_OPEN_TOUR").as_deref() != Ok("1") {
        return;
    }
    let _group = tracing::info_span!("external-open-tour-init").entered();
    let probe = if cfg!(windows) {
        "probe.bat"
    } else {
        "probe.sh"
    };
    let (samples, link_created) = match create_samples(probe) {
        Ok(samples) => samples,
        Err(error) => {
            log::error!("could not create external open tour samples: {error}");
            return;
        }
    };
    tracing::debug!(link_created, "created tour samples");

    let cases = [
        ("Open HTTPS", Action::Url("https://example.com/".into())),
        (
            "Open mailto",
            Action::Url("mailto:yard-test@example.com?subject=YARD".into()),
        ),
        (
            "Reveal sample.txt",
            Action::Reveal(samples.join("sample.txt")),
        ),
        ("Reveal folder", Action::Reveal(samples.join("folder"))),
        ("Open sample.txt", Action::Open(samples.join("sample.txt"))),
        ("Open file URL", Action::Url("file:///etc/hosts".into())),
        (
            "Open JavaScript URL",
            Action::Url("javascript:alert(1)".into()),
        ),
        (
            "Open product URL",
            Action::Url(format!("{scheme}://external_open_tour/loop")),
        ),
        (
            "Open HTTPS with credentials",
            Action::Url("https://user:pass@example.com/".into()),
        ),
        ("Open probe", Action::Open(samples.join(probe))),
        ("Open link.txt", Action::Open(samples.join("link.txt"))),
        ("Open folder", Action::Open(samples.join("folder"))),
        (
            "Open missing.txt",
            Action::Open(samples.join("missing.txt")),
        ),
    ];
    let extension = ext.clone();
    ext.register(cx, "external_open_tour.card", &[], move |_, _, _, cx| {
        cx.new(|_| Card {
            extension: extension.clone(),
            cases: cases
                .iter()
                .map(|(label, action)| Case {
                    label,
                    action: action.clone(),
                    result: String::new(),
                })
                .collect(),
        })
        .into()
    });
    ext.place_root(cx, "external-open-tour-card", "external_open_tour.card");
    tracing::debug!("placed the external open tour card");
}

fn create_sample_directory() -> io::Result<PathBuf> {
    let temp = std::env::temp_dir();
    let pid = std::process::id();
    for index in 0..=u64::MAX {
        let samples = temp.join(format!("yard-external-open-tour-{pid}-{index}"));
        match fs::create_dir(&samples) {
            Ok(()) => return Ok(samples),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no available tour sample directory",
    ))
}

fn create_samples(probe_name: &str) -> io::Result<(PathBuf, bool)> {
    let samples = create_sample_directory()?;
    fs::write(samples.join("sample.txt"), "YARD external open tour\n")?;
    fs::create_dir(samples.join("folder"))?;

    let probe = samples.join(probe_name);
    let link = samples.join("link.txt");
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};

        fs::write(&probe, "#!/bin/sh\nexit 0\n")?;
        fs::set_permissions(&probe, fs::Permissions::from_mode(0o755))?;
        Ok((samples, symlink(&probe, &link).is_ok()))
    }
    #[cfg(windows)]
    {
        fs::write(&probe, "@exit /b 0\r\n")?;
        Ok((
            samples,
            std::os::windows::fs::symlink_file(&probe, &link).is_ok(),
        ))
    }
}

#[derive(Clone)]
enum Action {
    Url(String),
    Reveal(PathBuf),
    Open(PathBuf),
}

impl Action {
    fn run(&self, ext: &Extension, cx: &App) -> Result<(), ExternalOpenError> {
        match self {
            Self::Url(url) => ext.open_url(url, cx),
            Self::Reveal(path) => ext.reveal_path(path, cx),
            Self::Open(path) => ext.open_path(path, cx),
        }
    }
}

struct Case {
    label: &'static str,
    action: Action,
    result: String,
}

struct Card {
    extension: Extension,
    cases: Vec<Case>,
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = palette(cx);
        v_flex()
            .id("external-open-tour")
            .size_full()
            .p_2()
            .gap_2()
            .border_1()
            .border_color(colors.border)
            .overflow_y_scroll()
            .children(self.cases.iter().enumerate().map(|(index, case)| {
                let view = cx.weak_entity();
                v_flex()
                    .flex_none()
                    .child(
                        Button::new(("external-open-case", index), case.label).on_click(
                            move |_, cx| {
                                let _ = view.update(cx, |card, cx| {
                                    let case = &mut card.cases[index];
                                    case.result = match case.action.run(&card.extension, cx) {
                                        Ok(()) => "opened".into(),
                                        Err(error) => error.to_string(),
                                    };
                                    cx.notify();
                                });
                            },
                        ),
                    )
                    .child(Label::new(case.result.clone()))
            }))
    }
}
