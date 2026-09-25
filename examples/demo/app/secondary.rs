//! The demo's secondary windows: a non-modal "Preferences" tool window with its
//! own counter message type, and a modal "Confirm" dialog returning a `bool`.
//!
//! Each runs its own `App` with its own `Msg` and its own queue while sharing
//! the process's single message loop; neither `update` is ever re-entered. The
//! [`open_prefs`], [`open_confirm`] and [`run_screenshot`] helpers keep all of
//! this wiring out of the main `app` module.

use std::path::PathBuf;

use win32ui::column;
use win32ui::prelude::*;

use super::screenshot;
use super::settings::SettingsPage;

/// A non-modal window's message type: its own counter, unrelated to the main
/// app's `Msg`.
pub(crate) enum PrefsMsg {
    Increment,
    /// The window is closing; hide it instead of destroying its state.
    Hide,
    /// Show the hidden window again, keeping its widgets and state.
    Show,
}

/// The "Preferences" tool window: a counter, demonstrated through a label and a
/// toolbar whose buttons map to [`PrefsMsg`].
pub(crate) struct PrefsApp {
    count: u32,
    label: Label,
    // Kept alive: the toolbar owns its child window and would be destroyed (and
    // vanish) if it were dropped when `new` returns.
    _toolbar: Toolbar<PrefsMsg>,
    // The settings page is much taller than the window, so it lives in a
    // `ScrollView`: a native, themed vertical scrollbar scrolls it.
    _scroll: ScrollView,
    _settings: Custom<SettingsPage, PrefsMsg>,
}

impl PrefsApp {
    pub(crate) fn new(ui: &mut Ui<PrefsMsg>) -> PrefsApp {
        let label = Label::new(ui, Rect::default(), "Counter: 0").expect("prefs label");
        let toolbar = Toolbar::new(
            ui,
            vec![
                ToolbarItem::new("Count +1").on_click(|| Some(PrefsMsg::Increment)),
                ToolbarItem::new("Close").on_click(|| Some(PrefsMsg::Hide)),
            ],
        )
        .expect("prefs toolbar");

        let scroll = ScrollView::new(ui).expect("scroll view");
        let settings = Custom::new(ui, SettingsPage::new()).expect("settings page");
        scroll.set_content(&settings);
        // Start a little way down so a screenshot shows the scrollbar in use.
        scroll.scroll_to(dip(40.0).to_px(ui.dpi()));

        ui.set_layout(column![toolbar, label.height(dip(28.0)), scroll.fill(1)].spacing(dip(8.0)));
        // A secondary window the app keeps alive: the close request is
        // intercepted and hides it (the parent shows it again with
        // `WindowHandle::show`), so its state — the counter, the scroll
        // position — survives. This is the pattern a visualization window uses.
        ui.on_close(|| Some(PrefsMsg::Hide));
        PrefsApp {
            count: 0,
            label,
            _toolbar: toolbar,
            _scroll: scroll,
            _settings: settings,
        }
    }
}

impl App for PrefsApp {
    type Msg = PrefsMsg;

    fn update(&mut self, msg: PrefsMsg, ui: &mut Ui<PrefsMsg>) {
        match msg {
            PrefsMsg::Increment => {
                self.count += 1;
                self.label.set_text(&format!("Counter: {}", self.count));
            }
            PrefsMsg::Hide => ui.hide(),
            PrefsMsg::Show => ui.show(),
        }
    }
}

/// A modal window's message type.
enum ConfirmMsg {
    Yes,
    No,
    Capture,
}

/// What to photograph before a screenshot-mode modal closes.
struct Snapshot {
    prefs: WindowHandle<PrefsMsg>,
    dir: PathBuf,
    theme: Theme,
}

/// The "Confirm" modal dialog: closes with `Some(true)` / `Some(false)`, or
/// with a screenshot in the demo's screenshot mode.
struct ConfirmApp {
    _label: Label,
    // Kept alive for the same reason as `PrefsApp::_toolbar`.
    _toolbar: Toolbar<ConfirmMsg>,
    snapshot: Option<Snapshot>,
}

impl ConfirmApp {
    fn new(ui: &mut Ui<ConfirmMsg>) -> ConfirmApp {
        Self::build(ui, None)
    }

    /// A modal that captures itself and `prefs` (both still open) into `dir`
    /// before closing, for the light/dark screenshots.
    fn new_screenshot(
        ui: &mut Ui<ConfirmMsg>,
        prefs: WindowHandle<PrefsMsg>,
        dir: PathBuf,
        theme: Theme,
    ) -> ConfirmApp {
        Self::build(ui, Some(Snapshot { prefs, dir, theme }))
    }

    fn build(ui: &mut Ui<ConfirmMsg>, snapshot: Option<Snapshot>) -> ConfirmApp {
        let label = Label::new(ui, Rect::default(), "Delete the 3 selected messages?")
            .expect("confirm label");
        let toolbar = Toolbar::new(
            ui,
            vec![
                ToolbarItem::new("Yes").on_click(|| Some(ConfirmMsg::Yes)),
                ToolbarItem::new("No").on_click(|| Some(ConfirmMsg::No)),
            ],
        )
        .expect("confirm toolbar");
        ui.set_layout(column![label.height(dip(28.0)), toolbar].spacing(dip(8.0)));
        if snapshot.is_some() {
            // The capture runs in `update`, after the window is shown.
            ui.emit(ConfirmMsg::Capture);
        }
        ConfirmApp {
            _label: label,
            _toolbar: toolbar,
            snapshot,
        }
    }
}

impl App for ConfirmApp {
    type Msg = ConfirmMsg;

    fn update(&mut self, msg: ConfirmMsg, ui: &mut Ui<ConfirmMsg>) {
        match msg {
            ConfirmMsg::Yes => ui.close_with_result(true),
            ConfirmMsg::No => ui.close_with_result(false),
            ConfirmMsg::Capture => {
                if let Some(snapshot) = &self.snapshot {
                    let prefs = snapshot.prefs.capture();
                    let dialog = ui.capture();
                    if let (Ok(prefs), Ok(dialog)) = (prefs, dialog) {
                        screenshot::write_composite(
                            &snapshot.dir,
                            snapshot.theme,
                            &[prefs, dialog],
                        );
                    }
                }
                ui.close_with_result(true);
            }
        }
    }
}

/// Opens the non-modal "Preferences" tool window and returns its handle.
pub(crate) fn open_prefs<M: 'static>(ui: &mut Ui<M>) -> Result<WindowHandle<PrefsMsg>> {
    ui.open_window(
        WindowSpec::new("Preferences").size(dip(360.0), dip(160.0)),
        PrefsApp::new,
    )
}

/// Opens the modal "Confirm" dialog and returns its result.
pub(crate) fn open_confirm<M: 'static>(ui: &mut Ui<M>) -> Option<bool> {
    ui.open_modal(
        WindowSpec::new("Confirm").size(dip(360.0), dip(160.0)),
        ConfirmApp::new,
    )
}

/// Runs the screenshot mode: opens both windows, increments the counter once
/// and captures them (composited) into the directory named by
/// `WIN32UI_DEMO_SECONDARY_SCREENSHOT`. Returns whether a capture was written.
pub(crate) fn run_screenshot<M: 'static>(ui: &mut Ui<M>) -> bool {
    let Ok(dir) = std::env::var("WIN32UI_DEMO_SECONDARY_SCREENSHOT").map(PathBuf::from) else {
        return false;
    };
    if dir.as_os_str().is_empty() {
        return false;
    }

    let prefs = match open_prefs(ui) {
        Ok(prefs) => prefs,
        Err(_) => return false,
    };
    // Advance the counter so the captured Preferences window shows "Counter: 1",
    // not the freshly created "Counter: 0".
    let _ = prefs.send(PrefsMsg::Increment);

    let theme = ui.theme();
    let prefs_for_modal = prefs.clone();
    let dir_for_modal = dir.clone();
    let _result: Option<bool> = ui.open_modal(
        WindowSpec::new("Confirm").size(dip(360.0), dip(160.0)),
        move |ui| ConfirmApp::new_screenshot(ui, prefs_for_modal, dir_for_modal, theme),
    );
    true
}
