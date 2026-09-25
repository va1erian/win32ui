#![forbid(unsafe_code)]

//! Vector icons for the toolbar.
//!
//! A [`ToolbarIcon`] is either one of the small built-in geometric shapes, a
//! glyph from **Segoe Fluent Icons** (falling back to Segoe MDL2 Assets) drawn
//! with DirectWrite, a caller-supplied Direct2D [`Path`], or one of the named
//! built-in vector icons (reply, forward, archive, star, …). Every icon is
//! anti-aliased, scaled to the toolbar's DPI and tinted from the theme.

mod paths;

use std::cell::RefCell;
use std::rc::Rc;

use crate::color::Color;
use crate::d2d::{
    D2dCanvas, Font, FontSpec, Layout, LineJoin, Path, PointF, RectF, Rgba, Stroke, TextSystem,
};
use crate::units::dip;

/// The design box the built-in named paths are authored in, in
/// device-independent pixels; the toolbar scales a path from it to the icon
/// size.
pub(crate) const DESIGN: f32 = 16.0;

/// A small icon drawn on a toolbar button.
///
/// The geometric variants are drawn directly; [`ToolbarIcon::glyph`] uses
/// DirectWrite, and [`ToolbarIcon::path`] / [`ToolbarIcon::outline`] draw a
/// caller-supplied Direct2D path. The named variants below are crisp built-in
/// vectors, so they do not depend on a font being installed.
#[derive(Clone)]
pub enum ToolbarIcon {
    /// A filled circle.
    Circle,
    /// A chevron pointing down.
    Chevron,
    /// A check mark.
    Check,
    /// An arrow pointing right.
    Arrow,
    /// A close (X) mark.
    Close,
    /// A glyph from Segoe Fluent Icons (falling back to Segoe MDL2 Assets),
    /// drawn with DirectWrite.
    Glyph(char),
    /// A caller-supplied Direct2D path, drawn filled. The path is authored in a
    /// 16×16 device-independent-pixel box and scaled to the icon size.
    Path(Rc<Path>),
    /// A caller-supplied Direct2D path, drawn as an outline `f32` device-
    /// independent pixels wide (in the same 16×16 design box).
    Outline(Rc<Path>, f32),
    /// A reply arrow.
    Reply,
    /// A forward arrow.
    Forward,
    /// An archive box.
    Archive,
    /// A trash can.
    Delete,
    /// A star outline.
    Star,
    /// A filled star.
    StarFilled,
    /// A refresh ring.
    Refresh,
    /// A gear.
    Settings,
    /// A pencil.
    Compose,
    /// An envelope.
    Mail,
    /// An envelope with a check.
    MarkRead,
    /// An envelope with an unread dot.
    MarkUnread,
}

impl ToolbarIcon {
    /// A glyph from Segoe Fluent Icons, falling back to Segoe MDL2 Assets.
    pub fn glyph(glyph: char) -> ToolbarIcon {
        ToolbarIcon::Glyph(glyph)
    }

    /// A caller-supplied path, drawn filled.
    pub fn path(path: Rc<Path>) -> ToolbarIcon {
        ToolbarIcon::Path(path)
    }

    /// A caller-supplied path, drawn as an outline.
    pub fn outline(path: Rc<Path>, width: f32) -> ToolbarIcon {
        ToolbarIcon::Outline(path, width)
    }
}

impl std::fmt::Debug for ToolbarIcon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            ToolbarIcon::Circle => "Circle",
            ToolbarIcon::Chevron => "Chevron",
            ToolbarIcon::Check => "Check",
            ToolbarIcon::Arrow => "Arrow",
            ToolbarIcon::Close => "Close",
            ToolbarIcon::Glyph(_) => "Glyph",
            ToolbarIcon::Path(_) => "Path",
            ToolbarIcon::Outline(..) => "Outline",
            ToolbarIcon::Reply => "Reply",
            ToolbarIcon::Forward => "Forward",
            ToolbarIcon::Archive => "Archive",
            ToolbarIcon::Delete => "Delete",
            ToolbarIcon::Star => "Star",
            ToolbarIcon::StarFilled => "StarFilled",
            ToolbarIcon::Refresh => "Refresh",
            ToolbarIcon::Settings => "Settings",
            ToolbarIcon::Compose => "Compose",
            ToolbarIcon::Mail => "Mail",
            ToolbarIcon::MarkRead => "MarkRead",
            ToolbarIcon::MarkUnread => "MarkUnread",
        };
        f.write_str(name)
    }
}

/// How a resolved path is painted.
pub(crate) enum PathPaint {
    Fill,
    Stroke(f32),
}

/// A [`ToolbarIcon`] resolved for painting: a geometric shape, a glyph layout,
/// or a path with its paint style. Resolving happens at construction (never in
/// the paint path).
pub(crate) enum ResolvedIcon {
    /// A built-in geometric shape, drawn with canvas primitives.
    Shape(ToolbarIcon),
    /// A path drawn filled or stroked.
    Path { path: Rc<Path>, paint: PathPaint },
    /// A laid-out glyph.
    Glyph(Layout),
    /// No icon could be resolved (DirectWrite unavailable): a drawn placeholder.
    Missing,
}

/// The icon font: Segoe Fluent Icons, or Segoe MDL2 Assets as its fallback.
/// Resolved once per UI thread; `None` when DirectWrite is unavailable.
pub(crate) fn icon_font() -> Option<Font> {
    thread_local! {
        static FONT: RefCell<Option<Option<Font>>> = const { RefCell::new(None) };
    }
    FONT.with(|cell| {
        let mut slot = cell.borrow_mut();
        slot.get_or_insert_with(|| {
            let system = TextSystem::new().ok()?;
            let spec = FontSpec::new("Segoe Fluent Icons, Segoe MDL2 Assets", DESIGN);
            system.font(&spec).ok()
        })
        .clone()
    })
}

/// The label font (the system UI face) at the toolbar's design size. Resolved
/// once per UI thread; `None` when DirectWrite is unavailable.
pub(crate) fn label_font() -> Option<Font> {
    thread_local! {
        static FONT: RefCell<Option<Option<Font>>> = const { RefCell::new(None) };
    }
    FONT.with(|cell| {
        let mut slot = cell.borrow_mut();
        slot.get_or_insert_with(|| {
            let system = TextSystem::new().ok()?;
            let spec = FontSpec::new("system-ui, Segoe UI, sans-serif", LABEL_SIZE);
            system.font(&spec).ok()
        })
        .clone()
    })
}

/// The label design size, in device-independent pixels.
pub(crate) const LABEL_SIZE: f32 = 12.0;

/// Resolves `icon` for painting. A glyph falls back to a drawn placeholder when
/// the icon font cannot be resolved.
pub(crate) fn resolve(icon: &ToolbarIcon) -> ResolvedIcon {
    match icon {
        ToolbarIcon::Circle
        | ToolbarIcon::Chevron
        | ToolbarIcon::Check
        | ToolbarIcon::Arrow
        | ToolbarIcon::Close => ResolvedIcon::Shape(icon.clone()),
        ToolbarIcon::Glyph(glyph) => icon_font()
            .and_then(|font| font.layout(&glyph.to_string(), f32::INFINITY).ok())
            .map_or(ResolvedIcon::Missing, ResolvedIcon::Glyph),
        ToolbarIcon::Path(path) => ResolvedIcon::Path {
            path: Rc::clone(path),
            paint: PathPaint::Fill,
        },
        ToolbarIcon::Outline(path, width) => ResolvedIcon::Path {
            path: Rc::clone(path),
            paint: PathPaint::Stroke(*width),
        },
        named => match named_path(named) {
            Some((path, paint)) => ResolvedIcon::Path { path, paint },
            None => ResolvedIcon::Missing,
        },
    }
}

/// Builds the path of a named icon, or `None` for a variant that has none.
fn named_path(icon: &ToolbarIcon) -> Option<(Rc<Path>, PathPaint)> {
    let (result, paint) = match icon {
        ToolbarIcon::Reply => (paths::reply(), PathPaint::Stroke(1.5)),
        ToolbarIcon::Forward => (paths::forward(), PathPaint::Stroke(1.5)),
        ToolbarIcon::Archive => (paths::archive(), PathPaint::Stroke(1.5)),
        ToolbarIcon::Delete => (paths::delete(), PathPaint::Stroke(1.5)),
        ToolbarIcon::Star => (paths::star(), PathPaint::Stroke(1.5)),
        ToolbarIcon::StarFilled => (paths::star_filled(), PathPaint::Fill),
        ToolbarIcon::Refresh => (paths::refresh(), PathPaint::Stroke(1.5)),
        ToolbarIcon::Settings => (paths::settings(), PathPaint::Stroke(1.5)),
        ToolbarIcon::Compose => (paths::compose(), PathPaint::Stroke(1.2)),
        ToolbarIcon::Mail => (paths::mail(), PathPaint::Stroke(1.5)),
        ToolbarIcon::MarkRead => (paths::mark_read(), PathPaint::Stroke(1.5)),
        ToolbarIcon::MarkUnread => (paths::mark_unread(), PathPaint::Stroke(1.5)),
        _ => return None,
    };
    result.ok().map(|path| (Rc::new(path), paint))
}

/// Draws `icon` in the square `rect` in `color`.
pub(crate) fn draw(canvas: &mut D2dCanvas<'_>, icon: &ResolvedIcon, rect: RectF, color: Color) {
    match icon {
        ResolvedIcon::Shape(shape) => draw_shape(canvas, shape, rect, color),
        ResolvedIcon::Glyph(layout) => {
            let (width, height) = layout.size();
            let x = rect.left + (rect.width() - width) / 2.0;
            let y = rect.top + (rect.height() - height) / 2.0;
            canvas.draw_text(layout, PointF::new(x, y), color);
        }
        ResolvedIcon::Path { path, paint } => {
            let scale = rect.width().min(rect.height()) / DESIGN;
            canvas.set_scale_translate(scale, rect.left, rect.top);
            match paint {
                PathPaint::Fill => canvas.fill_path(path, rgba(color)),
                PathPaint::Stroke(width) => canvas.stroke_path(
                    path,
                    rgba(color),
                    Stroke::solid(*width)
                        .join(LineJoin::Round)
                        .cap(crate::d2d::Cap::Round),
                ),
            }
            canvas.set_translation(0.0, 0.0);
        }
        ResolvedIcon::Missing => {
            let inner = RectF::new(
                rect.left + 2.0,
                rect.top + 2.0,
                rect.right - 2.0,
                rect.bottom - 2.0,
            );
            let stroke = Stroke::solid(1.5).dash(crate::d2d::DashStyle::Dashed);
            canvas.stroke_rounded_rect(inner, 2.0, color, stroke);
        }
    }
}

/// Draws one of the built-in geometric shapes.
fn draw_shape(canvas: &mut D2dCanvas<'_>, shape: &ToolbarIcon, rect: RectF, color: Color) {
    let stroke = (rect.width() / 8.0).max(1.0);
    let center_y = rect.top + rect.height() / 2.0;
    match shape {
        ToolbarIcon::Circle => {
            canvas.fill_ellipse(
                PointF::new(rect.left + rect.width() / 2.0, center_y),
                rect.width() / 2.0,
                rect.height() / 2.0,
                color,
            );
        }
        ToolbarIcon::Chevron => {
            let top = rect.top + rect.height() / 4.0;
            let bottom = rect.bottom - rect.height() / 4.0;
            let middle = rect.left + rect.width() / 2.0;
            canvas.draw_line(
                PointF::new(rect.left, top),
                PointF::new(middle, bottom),
                color,
                Stroke::solid(stroke),
            );
            canvas.draw_line(
                PointF::new(middle, bottom),
                PointF::new(rect.right, top),
                color,
                Stroke::solid(stroke),
            );
        }
        ToolbarIcon::Check => {
            let left = rect.left + rect.width() / 6.0;
            let middle = rect.left + rect.width() * 2.0 / 5.0;
            let right = rect.right - rect.width() / 6.0;
            let top = rect.top + rect.height() / 4.0;
            let bottom = rect.bottom - rect.height() / 4.0;
            canvas.draw_line(
                PointF::new(left, center_y),
                PointF::new(middle, bottom),
                color,
                Stroke::solid(stroke),
            );
            canvas.draw_line(
                PointF::new(middle, bottom),
                PointF::new(right, top),
                color,
                Stroke::solid(stroke),
            );
        }
        ToolbarIcon::Arrow => {
            let left = rect.left + rect.width() / 6.0;
            let right = rect.right - rect.width() / 6.0;
            let head = rect.height() / 5.0;
            canvas.draw_line(
                PointF::new(left, center_y),
                PointF::new(right, center_y),
                color,
                Stroke::solid(stroke),
            );
            canvas.draw_line(
                PointF::new(right - head, center_y - head),
                PointF::new(right, center_y),
                color,
                Stroke::solid(stroke),
            );
            canvas.draw_line(
                PointF::new(right, center_y),
                PointF::new(right - head, center_y + head),
                color,
                Stroke::solid(stroke),
            );
        }
        ToolbarIcon::Close => {
            let inset = rect.width() / 4.0;
            canvas.draw_line(
                PointF::new(rect.left + inset, rect.top + inset),
                PointF::new(rect.right - inset, rect.bottom - inset),
                color,
                Stroke::solid(stroke),
            );
            canvas.draw_line(
                PointF::new(rect.right - inset, rect.top + inset),
                PointF::new(rect.left + inset, rect.bottom - inset),
                color,
                Stroke::solid(stroke),
            );
        }
        _ => {}
    }
}

/// A token colour as the fully opaque [`Rgba`] Direct2D takes.
fn rgba(color: Color) -> Rgba {
    Rgba::with_alpha(color.r, color.g, color.b, 0xFF)
}

/// The design size of the toolbar icons, in device-independent pixels.
pub(crate) const ICON_SIZE: f32 = dip(16.0).value();
