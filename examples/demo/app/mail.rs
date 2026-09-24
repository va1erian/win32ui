//! The demo's Mail tab: two-line rows (sender + date over a dim subject) that
//! exercise `ListView::row_painter`, `row_style`, `row_height` and `zebra` —
//! the app decides row appearance instead of the control.

use win32ui::column;
use win32ui::gdi::TextFormat;
use win32ui::prelude::*;

use super::Msg;

/// One row of mock mail: an unread message gets a bold sender and an accent
/// bar, painted through `row_painter` rather than a "playing" concept.
pub(super) struct Mail {
    sender: String,
    subject: String,
    date: String,
    unread: bool,
}

fn mails() -> Vec<Mail> {
    let rows: &[(&str, &str, &str, bool)] = &[
        ("Aurora Fields", "Mix ready for review", "09:14", true),
        ("Junior State", "Re: tour dates", "Yesterday", false),
        ("Vela", "Artwork drafts attached", "Yesterday", true),
        ("The Midnight Set", "Invoice #1042", "Mon", false),
        ("Cassette Ghosts", "Studio time next week?", "Mon", false),
        ("山田 花子", "日本語のアルバム — final mix", "Sun", true),
        ("Junior State", "Newsletter: March round-up", "Sun", false),
    ];
    rows.iter()
        .map(|&(sender, subject, date, unread)| Mail {
            sender: sender.to_string(),
            subject: subject.to_string(),
            date: date.to_string(),
            unread,
        })
        .collect()
}

/// The Mail tab's list.
pub(super) struct MailTab {
    list: ListView<Mail, Msg>,
    caption: Label,
}

impl MailTab {
    pub(super) fn build(ui: &mut Ui<Msg>) -> MailTab {
        let theme = ui.theme();
        let list = ListView::new(ui)
            .expect("mail list")
            // One `Fill` column carries the whole row; `row_painter` draws
            // both lines and the accent bar into it itself.
            .column("Message", Fill, |row: &Mail| row.subject.as_str())
            .row_height(dip(44.0))
            .zebra(true)
            .row_style(move |row: &Mail| {
                let mut style = RowStyle::new().bold(row.unread);
                if row.unread {
                    style = style.accent_bar(theme.accent);
                }
                style
            })
            .row_painter(move |row: &Mail, canvas, rect, state| {
                let (background, sender_color) = if state.selected {
                    let background = if state.focused {
                        theme.selection
                    } else {
                        theme.selection_unfocused
                    };
                    (background, theme.text)
                } else {
                    let background = if state.alternate {
                        theme.background.lerp(theme.text, 0.04)
                    } else {
                        theme.background
                    };
                    (background, theme.text)
                };
                canvas.fill_rect(rect, background);
                if row.unread {
                    canvas.fill_rect(
                        Rect::new(rect.left, rect.top, rect.left + 3, rect.bottom),
                        theme.accent,
                    );
                }
                let text_left = rect.left + 10;
                let sender_line =
                    Rect::new(text_left, rect.top + 4, rect.right - 10, rect.top + 20);
                let subject_line =
                    Rect::new(text_left, rect.top + 22, rect.right - 10, rect.bottom - 4);
                canvas.draw_text(
                    sender_line,
                    &format!("{}    {}", row.sender, row.date),
                    sender_color,
                    TextFormat::left().single_line().end_ellipsis().no_prefix(),
                );
                canvas.draw_text(
                    subject_line,
                    &row.subject,
                    theme.text_secondary,
                    TextFormat::left().single_line().end_ellipsis().no_prefix(),
                );
                true
            })
            .on_activate(|item| Some(Msg::MailOpen(item)));
        list.set_model(mails());

        MailTab {
            list,
            caption: Label::new(ui, Rect::default(), "Select a message").expect("mail caption"),
        }
    }

    pub(super) fn page(&self) -> Layout {
        column![self.caption.height(dip(20.0)), self.list.fill(1),].spacing(dip(6.0))
    }

    /// Handles the tab's messages. Returns whether `msg` was one.
    pub(super) fn update(&mut self, msg: &Msg) -> bool {
        let Msg::MailOpen(item) = msg else {
            return false;
        };
        let text = match self.list.cell_text(*item, 0) {
            subject if subject.is_empty() => "Opened message".to_string(),
            subject => format!("Opened: {subject}"),
        };
        self.caption.set_text(&text);
        true
    }
}
