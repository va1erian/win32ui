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
    tint: CheckBox<Msg>,
    tint_caption: Label,
    tint_strength: Slider<Msg>,
    disabled: Button<Msg>,
    notes_label: Label,
    notes: Edit<Msg>,
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
        // The accent tint only shows on a window that has a material backdrop
        // and bands, so the demo turns it on with `WIN32UI_DEMO_ACCENT_TINT`.
        let tint_on = std::env::var_os("WIN32UI_DEMO_ACCENT_TINT").is_some();
        let tint = CheckBox::new(ui, "Accent tint")
            .expect("tint")
            .checked(tint_on)
            .on_toggle(|on| Some(Msg::AccentTint(on)));
        let tint_caption = Label::new(ui, Rect::default(), "Tint strength").expect("tint caption");
        let tint_strength = Slider::new(ui, 0.0..=255.0)
            .expect("tint strength")
            .value(f64::from(win32ui::DEFAULT_ACCENT_TINT_STRENGTH))
            .on_change(|value| Some(Msg::AccentStrength(value)));
        let disabled = Button::new(ui, "Disabled").expect("disabled");
        disabled.set_enabled(false);
        let notes_label = Label::new(ui, Rect::default(), "Notes").expect("notes label");
        let notes = Edit::multi_line(ui).expect("notes").word_wrap(true);
        notes.set_text("This is a multi-line edit.\nIt should have a thin border in dark mode.");

        Options {
            send,
            remote,
            themes,
            theme_group,
            tint,
            tint_caption,
            tint_strength,
            disabled,
            notes_label,
            notes,
        }
    }

    /// The panel's column of widgets.
    pub(super) fn page(&self) -> Layout {
        column![
            self.send,
            self.remote,
            self.theme_group.height(dip(20.0)),
            self.themes.layout(),
            self.tint,
            self.tint_caption.height(dip(18.0)),
            self.tint_strength.height(dip(24.0)),
            self.disabled,
            self.notes_label,
            self.notes.height(dip(80.0)),
        ]
        .spacing(dip(6.0))
    }

    /// Handles the options panel's messages. Returns whether `msg` was one.
    pub(super) fn update(
        &mut self,
        msg: &Msg,
        ui: &mut Ui<Msg>,
        status: &dyn super::StatusWriter,
    ) -> bool {
        match msg {
            Msg::Send => status.set_text(0, "Send clicked"),
            Msg::RemoteImages(on) => {
                ui.set_menu_checked(super::menus::REMOTE_IMAGES, *on);
                status.set_text(
                    0,
                    if *on {
                        "Remote images on"
                    } else {
                        "Remote images off"
                    },
                )
            }
            Msg::SetTheme(choice) => {
                // "System" hands theming over to the OS: `follow_system_theme`
                // applies `Theme::system()` now and again live, every time
                // `WM_SETTINGCHANGE`/`WM_SYSCOLORCHANGE`/`WM_THEMECHANGED`
                // reports a change (toggle Windows dark mode while the demo
                // runs to see it). The other two choices are a fixed palette,
                // so following is turned back off.
                ui.follow_system_theme(*choice == ThemeChoice::System);
                let next = match choice {
                    ThemeChoice::Light => Theme::light(),
                    ThemeChoice::Dark => Theme::dark(),
                    ThemeChoice::System => Theme::system(),
                };
                ui.set_theme(next);
                status.set_text(0, &format!("Theme: {choice:?}"));
            }
            Msg::ToggleTheme => {
                ui.follow_system_theme(false);
                let next = if ui.theme().is_dark {
                    Theme::light()
                } else {
                    Theme::dark()
                };
                ui.set_theme(next);
                status.set_text(0, "Theme switched");
            }
            Msg::AccentTint(on) => {
                ui.set_accent_tint(*on);
                status.set_text(
                    0,
                    if *on {
                        "Accent tint on"
                    } else {
                        "Accent tint off"
                    },
                );
            }
            Msg::AccentStrength(value) => {
                let strength = value.round().clamp(0.0, 255.0) as u8;
                ui.set_accent_tint_strength(strength);
                status.set_text(0, &format!("Tint strength {strength}"));
            }
            _ => return false,
        }
        true
    }
}
