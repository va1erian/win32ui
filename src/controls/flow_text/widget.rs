#![forbid(unsafe_code)]

//! The [`FlowText`](super::FlowText) widget: it styles its runs, lays them out
//! as one wrapped block, and turns hovering or clicking a link into the hand
//! cursor and the mapped message.

use std::cell::{Cell, RefCell};

use crate::controls::custom::{CustomWidget, Input, Renderer, WidgetCx};
use crate::d2d::{
    D2dCanvas, Font, FontSpec, PointF, RectF, Rgba, RichLayout, Span, Stroke, TextSystem,
    pixels_to_dips,
};
use crate::error::Result;
use crate::gdi::Canvas;
use crate::geometry::{Rect, Size};
use crate::message::MouseButton;
use crate::theme::Theme;
use crate::units::dip;
use crate::window::CursorShape;

use super::{Run, RunStyle};

/// The default family list, resolved entry by entry like any other font.
pub(super) const DEFAULT_FAMILY: &str = "system-ui, Segoe UI, Arial, sans-serif";
/// The default em size, in design units.
pub(super) const DEFAULT_SIZE: f32 = 14.0;
/// A placeholder natural width for the initial bounds, in design units.
const NATURAL_WIDTH: f32 = 240.0;
/// The hover underline thickness, in device-independent pixels.
const UNDERLINE: f32 = 1.0;

/// The cached layout: rebuilt only when the width or theme changes.
struct Cache {
    width: f32,
    theme: Theme,
    layout: RichLayout,
}

/// The runs with their theme colours resolved, ready to draw.
pub(super) struct FlowTextWidget<M> {
    text: TextSystem,
    font: RefCell<Font>,
    runs: RefCell<Vec<Run<M>>>,
    cache: RefCell<Option<Cache>>,
    /// The last theme painted, so input can hit-test before the next paint.
    theme: RefCell<Theme>,
    hover: Cell<Option<usize>>,
    pressed: Cell<Option<usize>>,
}

impl<M: 'static> FlowTextWidget<M> {
    pub(super) fn new() -> Result<FlowTextWidget<M>> {
        let text = TextSystem::new()?;
        let spec = FontSpec::new(DEFAULT_FAMILY, DEFAULT_SIZE);
        let font = text.font(&spec)?;
        Ok(FlowTextWidget {
            text,
            font: RefCell::new(font),
            runs: RefCell::new(Vec::new()),
            cache: RefCell::new(None),
            theme: RefCell::new(Theme::light()),
            hover: Cell::new(None),
            pressed: Cell::new(None),
        })
    }

    /// Appends a run and drops the cached layout.
    pub(super) fn push(&mut self, run: Run<M>) {
        self.runs.borrow_mut().push(run);
        *self.cache.borrow_mut() = None;
    }

    /// Replaces the base font runs without an explicit size inherit.
    pub(super) fn set_font(&self, family: &str, size_dip: f32) -> Result<()> {
        let font = self.text.font(&FontSpec::new(family, size_dip))?;
        *self.font.borrow_mut() = font;
        *self.cache.borrow_mut() = None;
        Ok(())
    }

    /// The last theme painted.
    pub(super) fn theme(&self) -> Theme {
        *self.theme.borrow()
    }

    /// The wrapped height of the runs at `width` device-independent pixels.
    pub(super) fn height_for(&self, width: f32, theme: &Theme) -> Result<f32> {
        Ok(self.build(width, theme)?.height())
    }

    /// Lays the runs out as one wrapped line at `width`, resolving each style
    /// to a theme token.
    fn build(&self, width: f32, theme: &Theme) -> Result<RichLayout> {
        let runs = self.runs.borrow();
        let spans: Vec<Span> = runs
            .iter()
            .map(|run| {
                let color = match run.style {
                    RunStyle::Normal => theme.text,
                    RunStyle::Weak => theme.text_secondary,
                    RunStyle::Link => theme.accent,
                };
                let span = Span::new(run.text.clone())
                    .weight(run.weight)
                    .italic(run.italic)
                    .color(Rgba::from(color));
                match run.size_dip {
                    Some(size) => span.size(size),
                    None => span,
                }
            })
            .collect();
        self.font.borrow().rich_layout(&spans, width)
    }

    /// Rebuilds the cache when `width` or `theme` moved on.
    fn ensure(&self, width: f32, theme: &Theme) {
        let fresh = self
            .cache
            .borrow()
            .as_ref()
            .is_some_and(|cache| cache.width == width && cache.theme == *theme);
        if fresh {
            return;
        }
        if let Ok(layout) = self.build(width, theme) {
            *self.cache.borrow_mut() = Some(Cache {
                width,
                theme: *theme,
                layout,
            });
        }
    }

    /// The index of the link run under `(x, y)`, if any.
    fn link_at(&self, x: f32, y: f32, width: f32, theme: &Theme) -> Option<usize> {
        self.ensure(width, theme);
        let cache = self.cache.borrow();
        let span = cache.as_ref()?.layout.hit_test_point(x, y).span;
        let runs = self.runs.borrow();
        (runs.get(span)?.style == RunStyle::Link).then_some(span)
    }

    /// Runs `run`'s click mapping, if it has one, and emits the message.
    fn activate(&self, run: usize, cx: &WidgetCx<M>) {
        let message = {
            let runs = self.runs.borrow();
            runs.get(run)
                .and_then(|run| run.on_click.as_ref())
                .and_then(|click| click())
        };
        if let Some(msg) = message {
            cx.emit(msg);
        }
    }

    fn underline(
        &self,
        canvas: &mut D2dCanvas<'_>,
        layout: &RichLayout,
        span: usize,
        theme: &Theme,
    ) {
        let stroke = Stroke::solid(UNDERLINE);
        for rect in layout.rects_of_span(span) {
            let y = (rect.bottom - UNDERLINE).max(rect.top);
            canvas.draw_line(
                PointF::new(rect.left, y),
                PointF::new(rect.right, y),
                theme.accent,
                stroke,
            );
        }
    }
}

impl<M: 'static> CustomWidget for FlowTextWidget<M> {
    type Event = M;

    /// Never called: the widget paints with Direct2D, and when Direct2D is
    /// unavailable the host fills the theme background instead.
    fn paint(&self, _canvas: &Canvas, _bounds: Rect, _theme: &Theme) {}

    fn renderer(&self) -> Renderer {
        Renderer::Direct2D
    }

    fn paint_d2d(&self, canvas: &mut D2dCanvas<'_>, bounds: RectF, theme: &Theme) {
        *self.theme.borrow_mut() = *theme;
        self.ensure(bounds.width(), theme);
        let cache = self.cache.borrow();
        let Some(cache) = cache.as_ref() else {
            return;
        };
        canvas.draw_rich_text(&cache.layout, PointF::new(0.0, 0.0));
        if let Some(span) = self.hover.get() {
            self.underline(canvas, &cache.layout, span, theme);
        }
    }

    fn preferred_size(&self, dpi: u32) -> Option<Size> {
        let height = self.font.borrow().metrics().line_height();
        Some(Size::new(
            dip(NATURAL_WIDTH).to_px(dpi).value(),
            dip(height).to_px(dpi).value(),
        ))
    }

    fn input(&self, input: Input, cx: &mut WidgetCx<M>) {
        let dpi = cx.dpi();
        let width = pixels_to_dips(cx.bounds().width(), dpi);
        let theme = self.theme();
        let point = |x: i32, y: i32| (pixels_to_dips(x, dpi), pixels_to_dips(y, dpi));
        match input {
            Input::MouseMove { x, y } => {
                let (x, y) = point(x, y);
                let link = self.link_at(x, y, width, &theme);
                cx.cursor(if link.is_some() {
                    CursorShape::Hand
                } else {
                    CursorShape::Arrow
                });
                if self.hover.get() != link {
                    self.hover.set(link);
                    cx.invalidate();
                }
            }
            Input::MouseDown {
                x,
                y,
                button: MouseButton::Left,
            } => {
                let (x, y) = point(x, y);
                self.pressed.set(self.link_at(x, y, width, &theme));
            }
            Input::MouseUp {
                x,
                y,
                button: MouseButton::Left,
            } => {
                let (x, y) = point(x, y);
                let released = self.link_at(x, y, width, &theme);
                if self.pressed.replace(None) == released
                    && let Some(run) = released
                {
                    self.activate(run, cx);
                }
            }
            Input::MouseLeave => {
                self.pressed.set(None);
                if self.hover.replace(None).is_some() {
                    cx.invalidate();
                }
            }
            Input::CaptureChanged => self.pressed.set(None),
            _ => {}
        }
    }
}
