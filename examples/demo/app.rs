//! Proof of concept for the `win32ui` widget layer: an `App` with a `Msg`
//! enum, an owner-drawn toolbar, a lazily-populated side tree and a virtual
//! (owner-data) list view.
//!
//! The window's features live in sibling modules under `app/`; this file is
//! wiring only: it builds them, installs the layout and routes each `Msg` to
//! the module that owns it.
//!
//! Run with:
//!
//! ```text
//! cargo run -p win32ui --example demo
//! ```

mod data;
mod dialogs;
mod document;
mod flow_text;
mod library;
mod menus;
mod options;
mod primitives;
mod screenshot;
mod search;
mod secondary;
mod settings;
mod slider;
mod swatch;
mod text_specimen;
mod toolbar;

use win32ui::prelude::*;
// `column!` is also a std prelude macro (an array helper), so the layout macros
// are imported explicitly to disambiguate.
use win32ui::{column, row, tabs};

use self::document::DocumentWidget;
use self::flow_text::Flow;
use self::library::{Library, SortKey};
use self::options::{Options, ThemeChoice};
use self::secondary::PrefsMsg;
use self::slider::Sliders;
use self::swatch::Swatch;

pub(crate) fn main() {
    // `WIN32UI_DEMO_THEME` / `WIN32UI_DEMO_WIDTH` / `WIN32UI_DEMO_HEIGHT` let a
    // screenshot run pick the palette and the window size without editing code.
    let initial = std::env::var("WIN32UI_DEMO_THEME").unwrap_or_else(|_| "dark".to_string());
    let theme = if initial.eq_ignore_ascii_case("light") {
        Theme::light()
    } else {
        Theme::dark()
    };
    let width = env_dip("WIN32UI_DEMO_WIDTH", 1080.0);
    let height = env_dip("WIN32UI_DEMO_HEIGHT", 680.0);
    // `WIN32UI_DEMO_BACKDROP=mica|mica-alt|acrylic` and
    // `WIN32UI_DEMO_TITLEBAR=colored` let a screenshot run exercise the
    // material and the themed caption without editing code.
    let backdrop = match std::env::var("WIN32UI_DEMO_BACKDROP").as_deref() {
        Ok("mica") => Backdrop::Mica,
        Ok("mica-alt") | Ok("mica_alt") => Backdrop::MicaAlt,
        Ok("acrylic") => Backdrop::Acrylic,
        _ => Backdrop::None,
    };
    let title_bar = match std::env::var("WIN32UI_DEMO_TITLEBAR").as_deref() {
        Ok("colored") => TitleBar::Colored,
        Ok("extended") => TitleBar::Extended,
        _ => TitleBar::Standard,
    };
    let result = win32ui::run_app(
        WindowSpec::new("win32ui demo")
            .size(dip(width), dip(height))
            .theme(theme)
            .backdrop(backdrop)
            .title_bar(title_bar),
        |ui| {
            let theme = ui.theme();
            text_specimen::open_if_requested(theme, ui.dpi());

            let toolbar = toolbar::build(ui, theme);
            let library = Library::build(ui);
            let status = StatusBar::new(ui).expect("status");
            status.set_parts(&[-1]);
            status.set_text(
                0,
                if ui.backdrop_active() {
                    "Ready — backdrop active"
                } else {
                    "Ready"
                },
            );
            let progress = ProgressBar::new(ui)
                .expect("progress")
                .range(0..=100)
                .value(40);
            // Every widget gets a tooltip through `ControlExt`, not just the
            // toolbar buttons.
            progress.set_tooltip("Scan progress");

            // A custom owner-drawn widget: a colour swatch that raises
            // `Clicked`, mapped to `Msg::SwatchClicked` below.
            let swatch = Custom::new(ui, Swatch::new(theme.accent))
                .expect("swatch")
                .on_event(|_| Some(Msg::SwatchClicked));

            // The Direct2D primitives panel (gradients, bitmaps, rounded
            // corners, clips and paths).
            let primitives = primitives::PrimitivesPanel::panel(ui);

            // A tall Direct2D document with its own vertical scroll host,
            // mapping the scroll offset to `Msg::DocumentScrolled`.
            let document = document::build(ui);

            // The demo's views live in one tab node — a draggable tree/list,
            // the Direct2D primitives, the document and the sliders — so the
            // window stays compact. `tabs!` pages layout subtrees; each page is
            // shown and hidden automatically and reports its index as a `Msg`.
            let library_page = library.page();
            let sliders = Sliders::build(ui);
            let sliders_page = sliders.page();
            let flow = Flow::build(ui);
            let views = tabs![
                ("Library", library_page),
                ("Primitives", primitives),
                ("Document", document),
                ("Sliders", sliders_page),
                ("Flow", flow.page()),
            ]
            // `WIN32UI_DEMO_TAB=3` opens the Sliders tab for a screenshot run.
            .selected(env_dip("WIN32UI_DEMO_TAB", 0.0) as usize)
            .on_change(|page| Some(Msg::TabsPage(page)));

            // Options panel: a default push button, a check box, a labelled
            // group of typed radios and a disabled button. The radios report
            // values, not indices.
            let options = Options::build(ui, theme);

            // A menu bar mapped to `Msg`; enabled items with a shortcut also
            // register that shortcut as an accelerator, so they fire while any
            // widget has focus.
            ui.set_menu_bar(menus::menu_bar(theme));

            // Shortcuts are data and fire whichever widget has focus. The menu
            // bar already auto-registers its items' shortcuts; these explicit
            // ones show the `accelerator` API and would be added by hand for
            // actions that have no menu item.
            ui.accelerator(Shortcut::ctrl(Key::Q), || Some(Msg::Quit));
            ui.accelerator(Shortcut::ctrl(Key::T), || Some(Msg::ToggleTheme));

            // The window owns the layout: it re-runs this tree on every resize
            // and DPI change, so the app never handles `WM_SIZE`.
            // An extended title bar reserves a top strip (the caption buttons
            // and menu bar); content starts below it so nothing sits under the
            // buttons. Zero on a standard title bar, so the layout is unchanged.
            let title_bar = ui.title_bar_height();
            ui.set_layout(
                column![
                    toolbar,
                    library.sort_row().height(dip(30.0)),
                    progress.height(dip(8.0)),
                    swatch.height(dip(24.0)),
                    row![views, options.page().width(dip(220.0))].fill(1),
                    status,
                ]
                .spacing(dip(4.0))
                .margins(Insets::new(dip(0.0), title_bar, dip(0.0), dip(0.0))),
            );

            let app = App {
                toolbar,
                library,
                status,
                progress,
                swatch,
                document,
                primitives,
                options,
                sliders,
                flow,
                prefs: None,
            };

            // A worker thread ticks a counter into the status bar through the
            // proxy. When the window is gone `send` hands the message back, so
            // the worker stops itself instead of panicking.
            let worker = ui.proxy();
            std::thread::spawn(move || {
                let mut tick = 0u64;
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    tick += 1;
                    if worker.send(Msg::Tick(tick)).is_err() {
                        break;
                    }
                }
            });

            // `WIN32UI_DEMO_AUTOCLOSE_MS` makes the demo quit itself; handy for
            // a headless smoke run of the example. `WIN32UI_DEMO_COMBO_OPEN`
            // drops the combo box's list down before the screenshot is taken.
            let auto_close = std::env::var("WIN32UI_DEMO_AUTOCLOSE_MS")
                .ok()
                .and_then(|millis| millis.parse().ok())
                .and_then(|millis| ui.set_timer(millis).ok());
            let combo_open = if std::env::var("WIN32UI_DEMO_COMBO_OPEN").is_ok() {
                ui.set_timer(1000).ok()
            } else {
                None
            };
            // `WIN32UI_DEMO_CONTEXT_OPEN` shows the list's context popup so a
            // dark (owner-drawn) menu can be inspected.
            let context_open = if std::env::var("WIN32UI_DEMO_CONTEXT_OPEN").is_ok() {
                ui.set_timer(1000).ok()
            } else {
                None
            };
            if auto_close.is_some() || combo_open.is_some() || context_open.is_some() {
                ui.on_timer(move |id| {
                    if Some(id) == auto_close {
                        Some(Msg::AutoClose)
                    } else if Some(id) == combo_open {
                        Some(Msg::OpenCombo)
                    } else if Some(id) == context_open {
                        Some(Msg::ShowListMenu)
                    } else {
                        None
                    }
                });
            }

            // `WIN32UI_DEMO_SECONDARY_SCREENSHOT` names a directory to write the
            // light/dark composite of both secondary window kinds to.
            if std::env::var("WIN32UI_DEMO_SECONDARY_SCREENSHOT").is_ok() {
                ui.emit(Msg::SecondaryScreenshot);
            }

            // `WIN32UI_DEMO_TAB` selects a tab page by index (0 = Library,
            // 1 = Primitives, 2 = Document) before a screenshot is taken.
            if let Ok(value) = std::env::var("WIN32UI_DEMO_TAB")
                && let Ok(index) = value.parse::<usize>()
            {
                ui.emit(Msg::TabsPage(index));
            }

            app
        },
    );
    if let Err(error) = result {
        eprintln!("demo failed: {error}");
        std::process::exit(1);
    }
}

/// A design-value size from an environment variable, or `default`.
fn env_dip(name: &str, default: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

enum Msg {
    Scan,
    Shuffle,
    Refresh,
    ToggleTheme,
    Clear,
    SplitMoved(Dip),
    TreeSelect,
    Play(usize),
    Selected(Vec<usize>),
    Sort(usize),
    Copy,
    Send,
    RemoteImages(bool),
    SetTheme(ThemeChoice),
    Tick(u64),
    SortChanged(SortKey),
    Search(String),
    OpenCombo,
    SwatchClicked,
    DocumentScrolled(Dip),
    OpenPrefs,
    OpenConfirm,
    SecondaryScreenshot,
    ShowListMenu,
    ContextPlay,
    ContextDelete,
    Quit,
    AutoClose,
    TabsPage(usize),
    Slider(slider::SliderMsg),
    Flow(flow_text::FlowMsg),
}

struct App {
    toolbar: Toolbar<Msg>,
    library: Library,
    status: StatusBar<Msg>,
    progress: ProgressBar,
    swatch: Custom<Swatch, Msg>,
    /// Owns the document widget's window; it paints and scrolls through its
    /// `HWND` and reports the offset as `Msg::DocumentScrolled`.
    #[allow(dead_code)]
    document: Custom<DocumentWidget, Msg>,
    // The Direct2D primitives panel (kept alive; only captured from).
    primitives: Custom<primitives::PrimitivesPanel, Msg>,
    options: Options,
    sliders: Sliders,
    flow: Flow,
    prefs: Option<WindowHandle<PrefsMsg>>,
}

impl App {
    fn set_status(&self, text: &str) {
        self.status.set_text(0, text);
    }
}

impl win32ui::App for App {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        // Each feature owns its messages; the first one to claim `msg` wins.
        if self.library.update(&msg, ui, &self.status) {
            return;
        }
        if self.options.update(&msg, ui, &self.status) {
            return;
        }
        if dialogs::update(&msg, ui, &self.status) {
            return;
        }
        if self.sliders.update(&msg, &self.status) {
            return;
        }
        if self.flow.update(&msg, &self.status) {
            return;
        }
        match msg {
            Msg::Scan => {
                self.progress.set_marquee(true);
                self.set_status("Scanning… (indeterminate)");
            }
            Msg::Shuffle => self.set_status("Shuffle requested"),
            Msg::Refresh => {
                self.toolbar.invalidate();
                self.progress.set_marquee(false);
                self.progress.set_range(0..=100);
                self.progress.set_value(40);
                self.set_status("Refreshed");
            }
            Msg::Tick(tick) => self.set_status(&format!("Worker tick {tick}")),
            Msg::SwatchClicked => {
                // Mutate the custom widget through its `Cell` state, then ask it
                // to repaint — the same pattern apps use for their own widgets.
                let theme = ui.theme();
                let next = match self.swatch.widget().borrow().color() {
                    c if c == theme.accent => theme.selection,
                    c if c == theme.selection => theme.warning,
                    _ => theme.accent,
                };
                self.swatch.widget().borrow().set_color(next);
                self.swatch.invalidate();
                self.set_status("Swatch clicked");
            }
            Msg::DocumentScrolled(offset) => {
                self.set_status(&format!("Document scrolled to {:.0} dip", offset.value()));
            }
            Msg::OpenPrefs => {
                if self.prefs.is_none() {
                    match secondary::open_prefs(ui) {
                        Ok(handle) => {
                            self.set_status("Preferences open");
                            self.prefs = Some(handle);
                        }
                        Err(error) => self.set_status(&format!("Preferences failed: {error}")),
                    }
                }
            }
            Msg::OpenConfirm => {
                let result = secondary::open_confirm(ui);
                self.set_status(&format!("Confirm returned {result:?}"));
            }
            Msg::SecondaryScreenshot => {
                self.set_status(if secondary::run_screenshot(ui) {
                    "Secondary windows captured"
                } else {
                    "Secondary screenshot failed"
                });
                ui.quit();
            }
            Msg::Quit => ui.quit(),
            Msg::TabsPage(page) => self.set_status(&format!("Tab page {page}")),
            Msg::AutoClose => {
                screenshot::capture_if_requested(ui);
                screenshot::capture_screen_if_requested(ui);
                primitives::PrimitivesPanel::capture_if_requested(ui, &self.primitives);
                ui.quit();
            }
            // Every other message was claimed by a feature module above.
            _ => {}
        }
    }
}
