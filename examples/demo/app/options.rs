//! The demo's options panel: a default push button, a check box, a labelled
//! group of typed theme radios and a disabled button, plus the theme messages
//! they raise.

use win32ui::column;
use win32ui::prelude::*;

use super::Msg;

/// The typed choices the options panel's radio group reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ThemeChoice {
    Light,
    Dark,
    System,
}

/// Handles for the options panel. The widgets paint and notify through their
/// `HWND`s; holding them here keeps those windows alive.
#[allow(dead_code)]
pub(super) struct Options {
    send: Button<Msg>,
    remote: CheckBox<Msg>,
    themes: RadioGroup<ThemeChoice, Msg>,
    theme_group: GroupBox,
    disabled: Button<Msg>,
}

impl Options {
    /// Builds the panel's widgets.
    pub(super) fn build(ui: &mut Ui<Msg>, theme: Theme) -> Options {
        let send = Button::new(ui, "Send")
            .expect("send")
            .default()
            .on_click(|| Some(Msg::Send));
        let remote = CheckBox::new(ui, "Load remote images")
            .expect("remote")
            .checked(false)
            .on_toggle(|on| Some(Msg::RemoteImages(on)));
        let theme_group = GroupBox::new(ui, "Theme").expect("theme group");
        let initial_choice = if theme.is_dark {
            ThemeChoice::Dark
        } else {
            ThemeChoice::Light
        };
        let themes = RadioGroup::new(
            ui,
            [
                ("Light", ThemeChoice::Light),
                ("Dark", ThemeChoice::Dark),
                ("System", ThemeChoice::System),
            ],
        )
        .expect("themes")
        .selected(initial_choice)
        .on_select(|choice| Some(Msg::SetTheme(*choice)));
        let disabled = Button::new(ui, "Disabled").expect("disabled");
        disabled.set_enabled(false);

        Options {
            send,
            remote,
            themes,
            theme_group,
            disabled,
        }
    }

    /// The panel's column of widgets.
    pub(super) fn page(&self) -> Layout {
        column![
            self.send,
            self.remote,
            self.theme_group.height(dip(20.0)),
            self.themes.layout(),
            self.disabled,
        ]
        .spacing(dip(6.0))
    }

    /// Handles the options panel's messages. Returns whether `msg` was one.
    pub(super) fn update(&mut self, msg: &Msg, ui: &mut Ui<Msg>, status: &StatusBar<Msg>) -> bool {
        match msg {
            Msg::Send => status.set_text(0, "Send clicked"),
            Msg::RemoteImages(on) => status.set_text(
                0,
                if *on {
                    "Remote images on"
                } else {
                    "Remote images off"
                },
            ),
            Msg::SetTheme(choice) => {
                // "System" follows the light palette until #23 lands.
                let next = if *choice == ThemeChoice::Dark {
                    Theme::dark()
                } else {
                    Theme::light()
                };
                ui.set_theme(next);
                status.set_text(0, &format!("Theme: {choice:?}"));
            }
            Msg::ToggleTheme => {
                let next = if ui.theme().is_dark {
                    Theme::light()
                } else {
                    Theme::dark()
                };
                ui.set_theme(next);
                status.set_text(0, "Theme switched");
            }
            _ => return false,
        }
        true
    }
}
