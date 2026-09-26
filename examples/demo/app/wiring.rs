//! Builds the demo window's widgets and installs its layout. Split out of
//! `app.rs` so that file stays wiring-only (module declarations, `main`, the
//! `Msg`/`App` types and the `update` dispatch).

use win32ui::prelude::*;
use win32ui::{column, row, tabs};

use super::document;
use super::flow_text::Flow;
use super::form::Form;
use super::gl_cube;
use super::grid::Grid;
use super::library::Library;
use super::mail::MailTab;
use super::menus;
use super::options::Options;
use super::primitives;
use super::slider::Sliders;
use super::swatch::Swatch;
use super::text_specimen;
use super::toolbar;
use super::topbar;
use super::{App, DemoStatus, Msg, env_dip};

/// Builds every widget, installs the layout, starts the demo timers and returns
/// the app state. This is the closure [`run_app`] calls.
pub(super) fn build(ui: &mut Ui<Msg>) -> App {
    let theme = ui.theme();
    // Install the window icon (resource 1 from win32ui.rc). The strip menu draws
    // it at the left of the caption row, before the title and menu items.
    if let Ok(icon) = Icon::from_resource(1) {
        ui.set_icon(icon);
    }
    text_specimen::open_if_requested(theme, ui.dpi());

    let toolbar = toolbar::build(ui, theme);
    let library = Library::build(ui);
    // `WIN32UI_DEMO_STATUS_MATERIAL=1` draws the status bar on the backdrop
    // material (bottom band) instead of in a child window.
    let status = if std::env::var_os("WIN32UI_DEMO_STATUS_MATERIAL").is_some() {
        match MaterialStatusBar::new(ui) {
            Ok(bar) => DemoStatus::Material(bar),
            Err(_) => DemoStatus::Child(StatusBar::new(ui).expect("status")),
        }
    } else {
        DemoStatus::Child(StatusBar::new(ui).expect("status"))
    };
    status.set_parts(&[-1]);
    // The status line starts with the attached monitors; `Ui::on_display_change`
    // maps a monitor layout change (a display plugged or unplugged) to
    // `Msg::MonitorsChanged`, which refreshes the same summary.
    status.set_text(0, &super::monitor_status(ui.backdrop_active()));
    ui.on_display_change(|| Some(Msg::MonitorsChanged));
    // `WIN32UI_DEMO_TOP_BAR=1` draws an interactive transport bar on the top
    // material band (only takes effect with `TitleBar::Extended`).
    let top_bar_on = std::env::var_os("WIN32UI_DEMO_TOP_BAR").is_some();
    let top_bar = if top_bar_on { topbar::build(ui) } else { None };
    let progress = ProgressBar::new(ui)
        .expect("progress")
        .range(0..=100)
        .value(40);
    // Every widget gets a tooltip through `ControlExt`, not just the toolbar
    // buttons.
    progress.set_tooltip("Scan progress");

    // A custom owner-drawn widget: a colour swatch that raises `Clicked`,
    // mapped to `Msg::SwatchClicked` below.
    let swatch = Custom::new(ui, Swatch::new(theme.accent))
        .expect("swatch")
        .on_event(|_| Some(Msg::SwatchClicked));

    // The Direct2D primitives panel (gradients, bitmaps, rounded corners,
    // clips and paths).
    let primitives = primitives::PrimitivesPanel::panel(ui);

    // A tall Direct2D document with its own vertical scroll host, mapping the
    // scroll offset to `Msg::DocumentScrolled`.
    let document = document::build(ui);

    // The demo's views live in one tab node — a draggable tree/list, the
    // Direct2D primitives, the document and the sliders — so the window stays
    // compact. `tabs!` pages layout subtrees; each page is shown and hidden
    // automatically and reports its index as a `Msg`.
    let library_page = library.page();
    let mail = MailTab::build(ui);
    let mail_page = mail.page();
    let sliders = Sliders::build(ui);
    let sliders_page = sliders.page();
    let flow = Flow::build(ui);
    let grid = Grid::build(ui);
    let form = Form::build(ui);
    let cube = gl_cube::Cube::build(ui);
    let views = tabs![
        ("Library", library_page),
        ("Mail", mail_page),
        ("Primitives", primitives),
        ("Document", document),
        ("Sliders", sliders_page),
        ("Flow", flow.page()),
        ("Grid", grid.page()),
        ("Form", form.page()),
        ("Cube", cube.page()),
    ]
    // `WIN32UI_DEMO_TAB=3` opens the Sliders tab for a screenshot run.
    .initial(env_dip("WIN32UI_DEMO_TAB", 0.0) as usize)
    .on_change(|page| Some(Msg::TabsPage(page)));

    // Options panel: a default push button, a check box, a labelled group of
    // typed radios and a disabled button. The radios report values, not
    // indices.
    let options = Options::build(ui, theme);

    // A menu bar mapped to `Msg`; enabled items with a shortcut also register
    // that shortcut as an accelerator, so they fire while any widget has focus.
    ui.set_menu_bar(menus::menu_bar(theme));

    // Shortcuts are data and fire whichever widget has focus. The menu bar
    // already auto-registers its items' shortcuts; these explicit ones show the
    // `accelerator` API and would be added by hand for actions that have no
    // menu item.
    ui.accelerator(Shortcut::ctrl(Key::Q), || Some(Msg::Quit));
    ui.accelerator(Shortcut::ctrl(Key::T), || Some(Msg::ToggleTheme));

    // The window owns the layout: it re-runs this tree on every resize and DPI
    // change, so the app never handles `WM_SIZE`.
    // An extended title bar reserves its strip and menu bar; zero otherwise.
    let title_bar = ui.title_bar_height();
    let views_row = row![views, options.page().width(dip(220.0))].fill(1);
    let layout = match &status {
        DemoStatus::Child(child) => column![
            toolbar,
            library.sort_row().height(dip(30.0)),
            progress.height(dip(8.0)),
            swatch.height(dip(24.0)),
            views_row,
            child.layout_item(),
        ]
        .spacing(dip(4.0))
        .margins(Insets::new(dip(0.0), title_bar, dip(0.0), dip(0.0))),
        DemoStatus::Material(_) => {
            // The material status bar is not a child: reserve its band with a
            // bottom margin instead of placing a widget.
            let bottom = ui.material_status_bar_height();
            column![
                toolbar,
                library.sort_row().height(dip(30.0)),
                progress.height(dip(8.0)),
                swatch.height(dip(24.0)),
                views_row,
            ]
            .spacing(dip(4.0))
            .margins(Insets::new(dip(0.0), title_bar, dip(0.0), bottom))
        }
    };
    ui.set_layout(layout);

    // `WIN32UI_DEMO_FULLSCREEN=1` makes the demo borderless fullscreen on the
    // primary monitor and auto-hides the idle cursor. An app toggles this at
    // runtime with `Ui::enter_fullscreen`/`leave_fullscreen`; here an env var
    // keeps the headline demo non-interactive for screenshots.
    if std::env::var_os("WIN32UI_DEMO_FULLSCREEN").is_some() {
        if let Some(primary) = monitors().into_iter().find(|monitor| monitor.primary) {
            let _ = ui.enter_fullscreen(&primary);
        }
        let _ = ui.hide_cursor_when_idle(2000);
    }

    let app = App {
        toolbar,
        library,
        mail,
        status,
        progress,
        swatch,
        document,
        primitives,
        options,
        sliders,
        flow,
        grid,
        form,
        cube,
        prefs: None,
        top_bar,
    };

    // A worker thread ticks a counter into the status bar through the proxy.
    // When the window is gone `send` hands the message back, so the worker
    // stops itself instead of panicking.
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

    // `WIN32UI_DEMO_AUTOCLOSE_MS` makes the demo quit itself; handy for a
    // headless smoke run of the example. `WIN32UI_DEMO_COMBO_OPEN` drops the
    // combo box's list down before the screenshot is taken.
    let auto_close = std::env::var("WIN32UI_DEMO_AUTOCLOSE_MS")
        .ok()
        .and_then(|millis| millis.parse().ok())
        .and_then(|millis| ui.set_timer(millis).ok());
    let combo_open = if std::env::var("WIN32UI_DEMO_COMBO_OPEN").is_ok() {
        ui.set_timer(1000).ok()
    } else {
        None
    };
    // `WIN32UI_DEMO_CONTEXT_OPEN` shows the list's context popup so a dark
    // (owner-drawn) menu can be inspected.
    let context_open = if std::env::var("WIN32UI_DEMO_CONTEXT_OPEN").is_ok() {
        ui.set_timer(1000).ok()
    } else {
        None
    };
    // `WIN32UI_DEMO_MAXIMIZE=1` maximizes the window after a moment, to
    // exercise the material surface's resize path.
    let maximize = if std::env::var_os("WIN32UI_DEMO_MAXIMIZE").is_some() {
        ui.set_timer(1200).ok()
    } else {
        None
    };
    if auto_close.is_some() || combo_open.is_some() || context_open.is_some() || maximize.is_some()
    {
        ui.on_timer(move |id| {
            if Some(id) == auto_close {
                Some(Msg::AutoClose)
            } else if Some(id) == combo_open {
                Some(Msg::OpenCombo)
            } else if Some(id) == context_open {
                Some(Msg::ShowListMenu)
            } else if Some(id) == maximize {
                Some(Msg::Maximize)
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

    // An inactive window draws differently, so a screen capture raises it.
    if std::env::var_os("WIN32UI_DEMO_SCREENSHOT_SCREEN").is_some() {
        ui.emit(Msg::Foreground);
    }

    // `WIN32UI_DEMO_TAB` picks a tab (0 Library, 1 Primitives, 2 Document).
    if let Ok(value) = std::env::var("WIN32UI_DEMO_TAB")
        && let Ok(index) = value.parse::<usize>()
    {
        ui.emit(Msg::TabsPage(index));
    }

    app
}
