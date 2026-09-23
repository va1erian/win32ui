//! A tall, owner-drawn "settings page" used as the demo's [`ScrollView`]
//! content: it is deliberately taller than the Preferences window so the
//! native, themed scrollbar has something to scroll.

use win32ui::Size;
use win32ui::gdi::{Canvas, TextFormat};
use win32ui::prelude::*;

/// The settings page's event type; it is display-only, so it raises nothing.
pub(crate) type SettingsEvent = ();

/// A list of labelled rows painted from the theme's tokens.
pub(crate) struct SettingsPage {
    rows: Vec<(&'static str, &'static str)>,
}

impl SettingsPage {
    pub(crate) fn new() -> SettingsPage {
        SettingsPage {
            rows: vec![
                ("Appearance", "Follow the system theme"),
                ("Language", "English (United Kingdom)"),
                ("Playback", "Crossfade tracks"),
                ("Equalizer", "Flat"),
                ("Library", "Watch folders for changes"),
                ("Downloads", "Ask where to save"),
                ("Network", "Use proxy settings"),
                ("Storage", "Keep a local cache"),
                ("Privacy", "Send anonymous statistics"),
                ("Updates", "Install automatically"),
                ("Advanced", "Show developer options"),
                ("About", "win32ui demo 0.1"),
            ],
        }
    }
}

impl CustomWidget for SettingsPage {
    type Event = SettingsEvent;

    fn paint(&self, canvas: &Canvas, bounds: Rect, theme: &Theme) {
        canvas.fill_rect(bounds, theme.background);
        let row_height = bounds.height().max(1) / self.rows.len() as i32;
        for (index, (name, value)) in self.rows.iter().enumerate() {
            let top = bounds.top + index as i32 * row_height;
            let row = Rect::new(bounds.left, top, bounds.right, top + row_height);
            canvas.fill_rect(
                Rect::new(row.left, row.bottom - 1, row.right, row.bottom),
                theme.border,
            );
            let text = Rect::new(row.left + 12, row.top, row.right / 2, row.bottom);
            canvas.draw_text(text, name, theme.text, TextFormat::left().vcenter());
            canvas.draw_text(
                Rect::new(row.right / 2, row.top, row.right - 12, row.bottom),
                value,
                theme.text_secondary,
                TextFormat::left().right().vcenter(),
            );
        }
    }

    fn preferred_size(&self, dpi: u32) -> Option<Size> {
        Some(Size::new(
            dip(320.0).to_px(dpi).value(),
            dip(520.0).to_px(dpi).value(),
        ))
    }
}
