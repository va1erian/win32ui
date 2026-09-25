#![forbid(unsafe_code)]

//! Built-in vector paths for the named [`ToolbarIcon`](super::ToolbarIcon)
//! variants. Each path is authored in a fixed 16×16 device-independent-pixel
//! box (see [`DESIGN`](super::DESIGN)); the toolbar scales it to the icon size,
//! so it stays crisp at every DPI.

use std::f32::consts::PI;

use crate::d2d::{ArcSize, Path, PathBuilder, PointF, Sweep};
use crate::error::Result;

use super::DESIGN;

/// A closed rectangle figure from its edges.
fn rect(builder: &mut PathBuilder, left: f32, top: f32, right: f32, bottom: f32) {
    builder
        .move_to(PointF::new(left, top))
        .line_to(PointF::new(right, top))
        .line_to(PointF::new(right, bottom))
        .line_to(PointF::new(left, bottom))
        .close();
}

/// The points of a five-pointed star centred in the design box.
fn star_points(center: f32, outer: f32, inner: f32) -> Vec<PointF> {
    (0..10)
        .map(|index| {
            let radius = if index % 2 == 0 { outer } else { inner };
            let angle = -PI / 2.0 + index as f32 * PI / 5.0;
            PointF::new(center + radius * angle.cos(), center + radius * angle.sin())
        })
        .collect()
}

/// A left-pointing reply arrow with a curved tail.
pub(super) fn reply() -> Result<Path> {
    let mut builder = PathBuilder::new()?;
    builder
        .move_to(PointF::new(6.5, 3.2))
        .line_to(PointF::new(2.2, 7.5))
        .line_to(PointF::new(6.5, 11.8));
    builder
        .move_to(PointF::new(2.2, 7.5))
        .line_to(PointF::new(9.5, 7.5))
        .quadratic_to(PointF::new(13.8, 7.5), PointF::new(13.8, 11.8));
    builder.build()
}

/// A right-pointing forward arrow with a curved tail.
pub(super) fn forward() -> Result<Path> {
    let mut builder = PathBuilder::new()?;
    builder
        .move_to(PointF::new(9.5, 3.2))
        .line_to(PointF::new(13.8, 7.5))
        .line_to(PointF::new(9.5, 11.8));
    builder
        .move_to(PointF::new(13.8, 7.5))
        .line_to(PointF::new(6.5, 7.5))
        .quadratic_to(PointF::new(2.2, 7.5), PointF::new(2.2, 11.8));
    builder.build()
}

/// An archive box: a lid and a body with a handle slot.
pub(super) fn archive() -> Result<Path> {
    let mut builder = PathBuilder::new()?;
    rect(&mut builder, 2.0, 3.0, 14.0, 6.2);
    builder
        .move_to(PointF::new(3.2, 6.2))
        .line_to(PointF::new(3.2, 13.0))
        .line_to(PointF::new(12.8, 13.0))
        .line_to(PointF::new(12.8, 6.2));
    builder
        .move_to(PointF::new(6.6, 9.0))
        .line_to(PointF::new(9.4, 9.0));
    builder.build()
}

/// A trash can: a lid, a handle and two ribs.
pub(super) fn delete() -> Result<Path> {
    let mut builder = PathBuilder::new()?;
    builder
        .move_to(PointF::new(3.0, 4.4))
        .line_to(PointF::new(13.0, 4.4));
    builder
        .move_to(PointF::new(6.6, 4.4))
        .line_to(PointF::new(6.6, 2.4))
        .line_to(PointF::new(9.4, 2.4))
        .line_to(PointF::new(9.4, 4.4));
    builder
        .move_to(PointF::new(4.6, 4.4))
        .line_to(PointF::new(5.2, 13.4))
        .line_to(PointF::new(10.8, 13.4))
        .line_to(PointF::new(11.4, 4.4));
    builder
        .move_to(PointF::new(7.1, 6.6))
        .line_to(PointF::new(7.1, 11.2));
    builder
        .move_to(PointF::new(8.9, 6.6))
        .line_to(PointF::new(8.9, 11.2));
    builder.build()
}

/// A five-pointed star outline.
pub(super) fn star() -> Result<Path> {
    let points = star_points(DESIGN / 2.0, 6.4, 2.7);
    let mut builder = PathBuilder::new()?;
    builder.move_to(points[0]);
    for point in &points[1..] {
        builder.line_to(*point);
    }
    builder.close();
    builder.build()
}

/// A filled five-pointed star.
pub(super) fn star_filled() -> Result<Path> {
    star()
}

/// A refresh ring: most of a circle with an arrow head.
pub(super) fn refresh() -> Result<Path> {
    let mut builder = PathBuilder::new()?;
    builder
        .move_to(PointF::new(8.0, 2.6))
        .arc_to(
            PointF::new(13.4, 8.0),
            5.4,
            5.4,
            Sweep::Clockwise,
            ArcSize::Small,
        )
        .arc_to(
            PointF::new(8.0, 13.4),
            5.4,
            5.4,
            Sweep::Clockwise,
            ArcSize::Small,
        )
        .arc_to(
            PointF::new(2.6, 8.0),
            5.4,
            5.4,
            Sweep::Clockwise,
            ArcSize::Small,
        );
    builder
        .move_to(PointF::new(8.0, 2.6))
        .line_to(PointF::new(4.8, 2.6))
        .move_to(PointF::new(8.0, 2.6))
        .line_to(PointF::new(8.0, 5.8));
    builder.build()
}

/// A gear: a central ring with eight teeth.
pub(super) fn settings() -> Result<Path> {
    let mut builder = PathBuilder::new()?;
    builder
        .move_to(PointF::new(11.0, 8.0))
        .arc_to(
            PointF::new(5.0, 8.0),
            3.0,
            3.0,
            Sweep::Clockwise,
            ArcSize::Small,
        )
        .arc_to(
            PointF::new(11.0, 8.0),
            3.0,
            3.0,
            Sweep::Clockwise,
            ArcSize::Small,
        );
    for index in 0..8 {
        let angle = index as f32 * PI / 4.0;
        let (sin, cos) = angle.sin_cos();
        builder
            .move_to(PointF::new(8.0 + 3.9 * cos, 8.0 + 3.9 * sin))
            .line_to(PointF::new(8.0 + 6.0 * cos, 8.0 + 6.0 * sin));
    }
    builder.build()
}

/// An outlined pencil: a body running from the top-right down to a pointed
/// tip, with a ferrule line across the flat end.
pub(super) fn compose() -> Result<Path> {
    let mut builder = PathBuilder::new()?;
    builder
        .move_to(PointF::new(11.4, 2.6))
        .line_to(PointF::new(13.6, 4.8))
        .line_to(PointF::new(4.8, 13.6))
        .line_to(PointF::new(2.6, 11.4))
        .close();
    builder
        .move_to(PointF::new(10.2, 3.8))
        .line_to(PointF::new(12.4, 6.0));
    builder
        .move_to(PointF::new(2.6, 11.4))
        .line_to(PointF::new(4.8, 13.6))
        .line_to(PointF::new(1.2, 14.4))
        .close();
    builder.build()
}

/// An envelope: a body and a flap.
pub(super) fn mail() -> Result<Path> {
    let mut builder = PathBuilder::new()?;
    rect(&mut builder, 2.0, 4.0, 14.0, 12.0);
    builder
        .move_to(PointF::new(2.0, 4.0))
        .line_to(PointF::new(8.0, 8.6))
        .line_to(PointF::new(14.0, 4.0));
    builder.build()
}

/// An envelope with a check mark.
pub(super) fn mark_read() -> Result<Path> {
    let mut builder = PathBuilder::new()?;
    rect(&mut builder, 2.0, 4.0, 14.0, 12.0);
    builder
        .move_to(PointF::new(2.0, 4.0))
        .line_to(PointF::new(8.0, 8.6))
        .line_to(PointF::new(14.0, 4.0));
    builder
        .move_to(PointF::new(5.4, 10.4))
        .line_to(PointF::new(7.2, 12.0))
        .line_to(PointF::new(10.8, 8.8));
    builder.build()
}

/// An envelope with an unread dot.
pub(super) fn mark_unread() -> Result<Path> {
    let mut builder = PathBuilder::new()?;
    rect(&mut builder, 2.0, 4.0, 14.0, 12.0);
    builder
        .move_to(PointF::new(2.0, 4.0))
        .line_to(PointF::new(8.0, 8.6))
        .line_to(PointF::new(14.0, 4.0));
    builder
        .move_to(PointF::new(6.7, 10.2))
        .line_to(PointF::new(8.0, 10.2))
        .arc_to(
            PointF::new(8.0, 11.5),
            0.65,
            0.65,
            Sweep::Clockwise,
            ArcSize::Small,
        )
        .arc_to(
            PointF::new(6.7, 11.5),
            0.65,
            0.65,
            Sweep::Clockwise,
            ArcSize::Small,
        )
        .close();
    builder.build()
}
