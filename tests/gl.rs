//! The OpenGL paint path: a `GlSurface` context on a window and a
//! `Renderer::Gl` widget drawing through it.
//!
//! Both tests need an OpenGL driver; when context creation fails (a remote or
//! headless session) they skip, because the widget deliberately falls back to
//! GDI there and there is nothing to assert. A watchdog makes a stuck message
//! loop fail instead of hang.

#![cfg(windows)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::column;
use win32ui::gdi::Canvas;
use win32ui::gl::GlSurface;
use win32ui::glow::{self, HasContext};
use win32ui::prelude::*;

/// A probe that records whether a frame was drawn and the driver's GL version.
#[derive(Clone, Default)]
struct Probe {
    version: Rc<RefCell<Option<String>>>,
    drew: Rc<Cell<bool>>,
}

impl Probe {
    fn note_version(&self, gl: &glow::Context) {
        if self.version.borrow().is_none() {
            // SAFETY: `gl` is current because a surface frame is active.
            let version = unsafe { gl.get_parameter_string(glow::VERSION) };
            *self.version.borrow_mut() = Some(version);
        }
    }
}

/// Whether this session can create an OpenGL context.
///
/// Probed on a hidden throwaway window: creating a context sets a GL pixel
/// format on its DC for good, which would break GDI repainting if it were the
/// app window's.
fn gl_available() -> bool {
    let Some(window) = open_probe_window() else {
        return false;
    };
    let available = GlSurface::new(window.hwnd()).is_ok();
    window.destroy();
    available
}

/// A hidden window to try OpenGL on, or `None` when the session cannot create
/// windows. The caller destroys it.
fn open_probe_window() -> Option<Window> {
    let class = WindowClass::register("win32ui.gl.probe", Theme::light().background).ok()?;
    Window::create(
        class,
        None,
        WindowStyle::overlapped(),
        WindowExStyle::new(),
        Rect::new(0, 0, 16, 16),
        "win32ui.gl.probe",
        common::NullHandler,
    )
    .ok()
}

struct SurfaceApp {
    probe: Probe,
}

impl App for SurfaceApp {
    type Msg = ();

    fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
        if let Some(window) = open_probe_window() {
            if let Ok(surface) = GlSurface::new(window.hwnd()) {
                let gl = surface.begin_frame(Color::rgb(0x10, 0x20, 0x30));
                self.probe.note_version(gl);
                // SAFETY: the surface made its context current for this frame.
                unsafe {
                    gl.clear_color(0.0, 0.0, 0.0, 1.0);
                    gl.clear(glow::COLOR_BUFFER_BIT);
                }
                self.probe.drew.set(surface.end_frame().is_ok());
            }
            window.destroy();
        }
        ui.quit();
    }
}

/// A `GlSurface` creates a context, begins a frame (viewport + clear) and
/// presents it. Skips when the session offers no OpenGL driver.
#[test]
fn gl_surface_draws_a_frame() {
    let probe = Probe::default();
    let probe_for_make = probe.clone();
    let Some(run) = run_app_with_watchdog("win32ui.gl.surface", move |ui| {
        ui.emit(());
        SurfaceApp {
            probe: probe_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    let Some(version) = probe.version.borrow().clone() else {
        return; // no OpenGL driver: the widget would fall back to GDI
    };
    assert!(!version.is_empty(), "GL_VERSION should not be empty");
    assert!(probe.drew.get(), "the GL frame was not presented");
}

/// A widget that opts into [`Renderer::Gl`] and counts its GL paints.
struct GlWidget {
    paints: Rc<Cell<u32>>,
    probe: Probe,
}

impl CustomWidget for GlWidget {
    type Event = ();

    fn paint(&self, _canvas: &Canvas, _bounds: Rect, _theme: &Theme) {}

    fn renderer(&self) -> Renderer {
        Renderer::Gl
    }

    fn paint_gl(&self, gl: &glow::Context, _bounds: Rect, _theme: &Theme) {
        self.paints.set(self.paints.get() + 1);
        self.probe.note_version(gl);
    }
}

enum GlMsg {
    Done,
}

struct GlApp {
    _widget: Custom<GlWidget, GlMsg>,
}

impl App for GlApp {
    type Msg = GlMsg;

    fn update(&mut self, msg: GlMsg, ui: &mut Ui<GlMsg>) {
        match msg {
            GlMsg::Done => ui.quit(),
        }
    }
}

/// A `Renderer::Gl` widget is painted with a current context. The frame is
/// captured only as a signal: when a GL context exists, `paint_gl` must have
/// run. Skips when the session offers no OpenGL driver.
#[test]
fn gl_widget_paints_through_its_context() {
    let paints = Rc::new(Cell::new(0u32));
    let probe = Probe::default();
    let available = Rc::new(Cell::new(false));

    let paints_for_make = Rc::clone(&paints);
    let probe_for_make = probe.clone();
    let available_for_make = Rc::clone(&available);
    let Some(run) = run_app_with_watchdog("win32ui.gl.widget", move |ui| {
        // Probe the driver once, so the assertion below can tell "no OpenGL"
        // from "the dispatch never reached `paint_gl`".
        available_for_make.set(gl_available());

        let widget = Custom::new(
            ui,
            GlWidget {
                paints: Rc::clone(&paints_for_make),
                probe: probe_for_make.clone(),
            },
        )
        .expect("gl widget");
        ui.set_layout(column![widget.fill(1)]);

        // The window paints on show; quit a moment later, after at least one
        // frame, so `paint_gl` has had a chance to run.
        let timer = ui.set_timer(300).ok();
        ui.on_timer(move |id| (Some(id) == timer).then_some(GlMsg::Done));
        GlApp { _widget: widget }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !available.get() {
        return; // no OpenGL driver: the widget fell back to GDI
    }
    assert!(
        paints.get() > 0,
        "a Renderer::Gl widget was never called with a current context"
    );
    assert!(
        probe.version.borrow().is_some(),
        "the widget did not receive a live glow context"
    );
}

/// A widget that records the GL context available at teardown and through
/// [`Custom::with_gl`], to prove both run with a context current.
struct LifecycleWidget {
    paints: Rc<Cell<u32>>,
    teardown_ran: Rc<Cell<bool>>,
    teardown_version: Rc<RefCell<Option<String>>>,
}

impl CustomWidget for LifecycleWidget {
    type Event = ();

    fn paint(&self, _canvas: &Canvas, _bounds: Rect, _theme: &Theme) {}

    fn renderer(&self) -> Renderer {
        Renderer::Gl
    }

    fn paint_gl(&self, _gl: &glow::Context, _bounds: Rect, _theme: &Theme) {
        self.paints.set(self.paints.get() + 1);
    }

    fn gl_teardown(&self, gl: &glow::Context) {
        self.teardown_ran.set(true);
        // SAFETY: the surface made its context current for this teardown.
        let version = unsafe { gl.get_parameter_string(glow::VERSION) };
        self.teardown_version.borrow_mut().replace(version);
    }
}

impl LifecycleWidget {
    fn new(
        paints: Rc<Cell<u32>>,
        teardown_ran: Rc<Cell<bool>>,
        teardown_version: Rc<RefCell<Option<String>>>,
    ) -> LifecycleWidget {
        LifecycleWidget {
            paints,
            teardown_ran,
            teardown_version,
        }
    }
}

/// An app that runs [`Custom::with_gl`] once the first frame has painted.
struct LifecycleApp {
    widget: Custom<LifecycleWidget, GlMsg>,
    with_gl_ran: Rc<Cell<bool>>,
    with_gl_version: Rc<RefCell<Option<String>>>,
}

impl App for LifecycleApp {
    type Msg = GlMsg;

    fn update(&mut self, msg: GlMsg, ui: &mut Ui<GlMsg>) {
        match msg {
            GlMsg::Done => {
                let version = self.widget.with_gl(|gl| {
                    // SAFETY: `with_gl` made the context current.
                    unsafe { gl.get_parameter_string(glow::VERSION) }
                });
                if let Some(version) = version {
                    self.with_gl_ran.set(true);
                    self.with_gl_version.borrow_mut().replace(version);
                }
                ui.quit();
            }
        }
    }
}

/// `gl_teardown` and `Custom::with_gl` both run with the widget's context
/// current. Skips when the session offers no OpenGL driver.
#[test]
fn gl_teardown_and_with_gl_run_with_a_current_context() {
    let paints = Rc::new(Cell::new(0u32));
    let teardown_ran = Rc::new(Cell::new(false));
    let teardown_version = Rc::new(RefCell::new(None));
    let with_gl_ran = Rc::new(Cell::new(false));
    let with_gl_version = Rc::new(RefCell::new(None));
    let available = Rc::new(Cell::new(false));

    let paints_for_make = Rc::clone(&paints);
    let teardown_for_make = Rc::clone(&teardown_ran);
    let teardown_version_for_make = Rc::clone(&teardown_version);
    let with_gl_for_make = Rc::clone(&with_gl_ran);
    let with_gl_version_for_make = Rc::clone(&with_gl_version);
    let available_for_make = Rc::clone(&available);
    let Some(run) = run_app_with_watchdog("win32ui.gl.lifecycle", move |ui| {
        available_for_make.set(gl_available());

        let widget = Custom::new(
            ui,
            LifecycleWidget::new(
                Rc::clone(&paints_for_make),
                teardown_for_make,
                teardown_version_for_make,
            ),
        )
        .expect("gl widget");
        ui.set_layout(column![widget.fill(1)]);

        let timer = ui.set_timer(300).ok();
        ui.on_timer(move |id| (Some(id) == timer).then_some(GlMsg::Done));
        LifecycleApp {
            widget,
            with_gl_ran: with_gl_for_make,
            with_gl_version: with_gl_version_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !available.get() {
        return; // no OpenGL driver: the widget fell back to GDI
    }
    assert!(paints.get() > 0, "the widget never painted through GL");
    assert!(
        with_gl_ran.get(),
        "Custom::with_gl did not run with a live context"
    );
    assert!(
        teardown_ran.get(),
        "gl_teardown was not called when the widget was destroyed"
    );
    assert!(
        with_gl_version
            .borrow()
            .as_deref()
            .is_some_and(|v| !v.is_empty()),
        "Custom::with_gl did not receive a live glow context"
    );
    assert!(
        teardown_version
            .borrow()
            .as_deref()
            .is_some_and(|v| !v.is_empty()),
        "gl_teardown did not receive a live glow context"
    );
}
