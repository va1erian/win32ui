//! A tall Direct2D document hosted in a custom widget with its built-in
//! vertical scroll host: rounded boxes and coloured bands that scroll with the
//! wheel, the keyboard and the thumb.
//!
//! This exercises [`CustomWidget::renderer`] (Direct2D) and
//! [`CustomWidget::paint_d2d`] together with [`Custom::with_vscroll`]. The
//! scroll offset maps to [`Msg::DocumentScrolled`](super::Msg::DocumentScrolled)
//! through [`Custom::on_scroll`].

use win32ui::d2d::{D2dCanvas, RectF};
use win32ui::prelude::*;

use super::Msg;

/// The document's content height, in design units.
const CONTENT_DIP: f32 = 2000.0;
/// The height of one colour band, in design units.
const BAND_DIP: f32 = 140.0;
/// The corner radius of the rounded boxes, in design units.
const BOX_RADIUS: f32 = 10.0;

/// The document widget: it only paints, so its state is immutable.
pub(super) struct DocumentWidget;

impl CustomWidget for DocumentWidget {
    type Event = ();

    fn paint(&self, _canvas: &win32ui::gdi::Canvas, _bounds: Rect, _theme: &Theme) {}

    fn renderer(&self) -> Renderer {
        Renderer::Direct2D
    }

    fn paint_d2d(&self, canvas: &mut D2dCanvas<'_>, bounds: RectF, theme: &Theme) {
        canvas.clear(theme.background);
        let width = bounds.width();
        let margin = 16.0;
        let mut y = 0.0;
        let mut band = 0usize;
        while y < CONTENT_DIP {
            let height = (BAND_DIP * 0.9).min(CONTENT_DIP - y);
            let fill = band_fill(band, theme);
            canvas.fill_rect(RectF::new(0.0, y, width, y + height), fill);
            // A rounded box inside the band, in a contrasting surface.
            let box_rect = RectF::new(margin, y + 18.0, width - margin, y + height - 18.0);
            canvas.fill_rounded_rect(box_rect, BOX_RADIUS, theme.raised);
            let accent = band_accent(band, theme);
            let strip = RectF::new(
                box_rect.left,
                box_rect.top,
                box_rect.left + 6.0,
                box_rect.bottom,
            );
            canvas.fill_rounded_rect(strip, BOX_RADIUS, accent);
            y += BAND_DIP;
            band += 1;
        }
    }
}

/// The fill colour for a band: alternating surfaces with occasional accent
/// bands, all semantic tokens so light/dark switching just works.
fn band_fill(band: usize, theme: &Theme) -> Color {
    match band % 5 {
        0 => theme.surface,
        1 => theme.hover,
        2 => theme.raised,
        3 => theme.selection_unfocused,
        _ => theme.surface,
    }
}

/// The accent strip colour for a band.
fn band_accent(band: usize, theme: &Theme) -> Color {
    match band % 4 {
        0 => theme.accent,
        1 => theme.warning,
        2 => theme.danger,
        _ => theme.selection,
    }
}

/// Builds the document widget, scrolling to `WIN32UI_DEMO_DOC_SCROLL` design
/// units when it is set (for a scrolled screenshot).
pub(super) fn build(ui: &mut Ui<Msg>) -> Custom<DocumentWidget, Msg> {
    let document = Custom::new(ui, DocumentWidget)
        .expect("document")
        .with_vscroll()
        .on_scroll(|offset| Some(Msg::DocumentScrolled(offset)));
    document.set_content_height(dip(CONTENT_DIP));
    if let Ok(value) = std::env::var("WIN32UI_DEMO_DOC_SCROLL")
        && let Ok(offset) = value.parse::<f32>()
    {
        document.scroll_to(dip(offset));
    }
    document
}
