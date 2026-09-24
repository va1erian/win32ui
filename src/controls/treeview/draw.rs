#![forbid(unsafe_code)]

//! The custom-draw hook behind [`TreeView`](super::TreeView): themed selection,
//! per-node colours, bold weight and trailing badge text.
//!
//! The native control still draws its own expand glyphs and icons; the hook
//! only recolours the row, selects the bold font when a node asks for it, and
//! paints the badge over the row's trailing edge.

use windows::Win32::UI::Controls::{CDDS_ITEMPOSTPAINT, CDDS_ITEMPREPAINT, CDDS_PREPAINT};

use std::hash::Hash;

use crate::controls::treeview::inner::TreeViewInner;
use crate::gdi::{Canvas, TextFormat};
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::sys;
use crate::sys::treeview::{CustomDraw, CustomDrawResult};

impl<K: Clone + Eq + Hash + 'static> TreeViewInner<K> {
    /// Handles one `NM_CUSTOMDRAW` stage.
    pub(crate) fn custom_draw(&self, hwnd: Hwnd, ctx: &mut CustomDraw) -> CustomDrawResult {
        if ctx.stage == CDDS_PREPAINT.0 {
            return CustomDrawResult::NotifyItemDraw;
        }
        if ctx.stage == CDDS_ITEMPREPAINT.0 {
            let Some(style) = self.styles.get(&ctx.token) else {
                return CustomDrawResult::Default;
            };
            let (background, text) = self.row_colors(hwnd, ctx.selected, ctx.hot);
            // Selection and hover win over a node's own background.
            ctx.background = background.or(style.background);
            ctx.text = text.or(style.text);
            if style.bold {
                let _ = sys::treeview::select_font(ctx.hdc, self.bold_font.raw());
            }
            if style.badge.is_some() {
                return CustomDrawResult::NewFontAndPostPaint;
            }
            return CustomDrawResult::NewFont;
        }
        if ctx.stage == CDDS_ITEMPOSTPAINT.0 {
            if let Some(badge) = self
                .styles
                .get(&ctx.token)
                .and_then(|style| style.badge.as_deref())
            {
                let canvas = Canvas::new(ctx.hdc);
                canvas.with_font(&self.font, |canvas| {
                    let width = canvas.text_size(badge).width + 8;
                    let rect = Rect::new(
                        ctx.rect.right - width,
                        ctx.rect.top,
                        ctx.rect.right - 2,
                        ctx.rect.bottom,
                    );
                    canvas.draw_text(
                        rect,
                        badge,
                        self.theme.text_secondary,
                        TextFormat::left()
                            .right()
                            .single_line()
                            .vcenter()
                            .no_prefix(),
                    );
                });
            }
            return CustomDrawResult::Default;
        }
        CustomDrawResult::Default
    }

    /// The background/text overrides for an unselected, selected or hovered
    /// row. `None` leaves the native control's own colour (the theme
    /// background/text set with `TVM_SETBKCOLOR`/`TVM_SETTEXTCOLOR`).
    fn row_colors(
        &self,
        hwnd: Hwnd,
        selected: bool,
        hot: bool,
    ) -> (Option<crate::Color>, Option<crate::Color>) {
        if selected {
            let background = if sys::treeview::tv_has_focus(hwnd) {
                self.theme.selection
            } else {
                self.theme.selection_unfocused
            };
            (Some(background), Some(self.theme.text))
        } else if hot {
            (Some(self.theme.hover), None)
        } else {
            (None, None)
        }
    }
}
