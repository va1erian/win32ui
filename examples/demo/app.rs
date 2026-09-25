//! Proof of concept for the `win32ui` widget layer: an `App` with a `Msg`
//! enum, an owner-drawn toolbar, a lazily-populated side tree and a virtual
//! (owner-data) list view.
//!
//! The window's features live in sibling modules under `app/`; this file is
//! wiring only: it parses the environment, builds the window and routes each
//! `Msg` to the module that owns it. The widget tree itself is assembled in
//! [`wiring`].
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
mod form;
mod gl_cube;
mod grid;
mod library;
mod mail;
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
mod topbar;
mod wiring;

use win32ui::prelude::*;

use self::document::DocumentWidget;
use self::flow_text::Flow;
use self::form::{Form, FormMsg};
use self::gl_cube::Cube;
use self::grid::Grid;
use self::library::{Library, SortKey};
use self::mail::MailTab;
use self::options::{Options, ThemeChoice};
use self::secondary::PrefsMsg;
use self::slider::Sliders;
use self::swatch::Swatch;

pub(crate) fn main() {
    let _restore_pointer = screenshot::park_pointer_if_requested();
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
    // `WIN32UI_DEMO_MENU_STRIP=1` draws the menu on the acrylic strip instead
    // of a native bar (only takes effect with the extended title bar and an
    // active material). `WIN32UI_DEMO_MENU_PLACEMENT=inline|stacked` picks
    // whether the items share the caption row or get a row below it.
    // The top bar sits below the strip's menu row, so it needs the self-drawn
    // strip menu rather than a native menu bar (which the transparent surface
    // would cover). Requesting the top bar therefore also puts the menu on the
    // strip.
    let top_bar_on = std::env::var_os("WIN32UI_DEMO_TOP_BAR").is_some();
    let menu_in_strip = std::env::var_os("WIN32UI_DEMO_MENU_STRIP").is_some() || top_bar_on;
    let menu_placement = match std::env::var("WIN32UI_DEMO_MENU_PLACEMENT").as_deref() {
        Ok("inline") => MenuStripPlacement::Inline,
        _ => MenuStripPlacement::Stacked,
    };
    let result = win32ui::run_app(
        WindowSpec::new("win32ui demo")
            .size(dip(width), dip(height))
            .theme(theme)
            .backdrop(backdrop)
            .title_bar(title_bar)
            .menu_in_strip(menu_in_strip)
            .menu_strip_placement(menu_placement),
        wiring::build,
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
    Reply,
    Forward,
    Archive,
    Star(bool),
    Compose,
    SplitMoved(Dip),
    TreeSelect(u32),
    TreeFold(u32, bool),
    Play(usize),
    MailOpen(usize),
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
    Foreground,
    Maximize,
    TabsPage(usize),
    Slider(slider::SliderMsg),
    Flow(flow_text::FlowMsg),
    Grid(grid::GridMsg),
    Form(FormMsg),
    TopBar(TopBarEvent),
}

/// The demo's status bar: a child `StatusBar`, or a `MaterialStatusBar` drawn
/// on the backdrop when `WIN32UI_DEMO_STATUS_MATERIAL` is set.
enum DemoStatus {
    Child(StatusBar<Msg>),
    Material(MaterialStatusBar<Msg>),
}

/// The status-text sink the feature modules write to, so they do not care
/// whether the bar is the child `StatusBar` or the material one.
trait StatusWriter {
    fn set_text(&self, part: usize, text: &str);
}

impl StatusWriter for DemoStatus {
    fn set_text(&self, part: usize, text: &str) {
        DemoStatus::set_text(self, part, text);
    }
}

impl DemoStatus {
    fn set_text(&self, part: usize, text: &str) {
        match self {
            DemoStatus::Child(child) => child.set_text(part, text),
            DemoStatus::Material(bar) => bar.set_text(part, text),
        }
    }

    fn set_parts(&self, edges: &[i32]) {
        match self {
            DemoStatus::Child(child) => child.set_parts(edges),
            DemoStatus::Material(bar) => bar.set_parts(edges),
        }
    }
}

struct App {
    toolbar: Toolbar<Msg>,
    library: Library,
    mail: MailTab,
    status: DemoStatus,
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
    grid: Grid,
    form: Form,
    /// The OpenGL cube tab (kept alive; it animates itself).
    #[allow(dead_code)]
    cube: Cube,
    prefs: Option<WindowHandle<PrefsMsg>>,
    /// The material transport bar, when `WIN32UI_DEMO_TOP_BAR` is set.
    top_bar: Option<topbar::TopBar>,
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
        if self.mail.update(&msg) {
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
        if self.grid.update(&msg) {
            return;
        }
        if self.form.update(&msg, &self.status) {
            return;
        }
        match msg {
            Msg::Scan => {
                self.progress.set_marquee(true);
                self.set_status("Scanning… (indeterminate)");
            }
            Msg::Shuffle => self.set_status("Shuffle requested"),
            Msg::Reply => self.set_status("Reply"),
            Msg::Forward => self.set_status("Forward"),
            Msg::Archive => self.set_status("Archived"),
            Msg::Star(checked) => self.set_status(if checked { "Starred" } else { "Unstarred" }),
            Msg::Compose => self.set_status("Compose"),
            Msg::Refresh => {
                self.toolbar.invalidate();
                self.progress.set_marquee(false);
                self.progress.set_range(0..=100);
                self.progress.set_value(40);
                self.set_status("Refreshed");
            }
            Msg::Tick(tick) => {
                if let Some(bar) = &self.top_bar {
                    bar.tick();
                }
                self.set_status(&format!("Worker tick {tick}"));
            }
            Msg::TopBar(event) => {
                let text = self.top_bar.as_mut().map(|bar| bar.event(event));
                if let Some(text) = text {
                    self.set_status(&text);
                }
            }
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
            Msg::Foreground => ui.set_foreground(),
            Msg::Maximize => {
                let hwnd =
                    windows::Win32::Foundation::HWND(ui.hwnd().raw() as *mut core::ffi::c_void);
                // SAFETY: `hwnd` is the live demo window; `SW_MAXIMIZE` is a
                // documented show command.
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
                        hwnd,
                        windows::Win32::UI::WindowsAndMessaging::SW_MAXIMIZE,
                    );
                }
            }

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
