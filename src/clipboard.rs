#![forbid(unsafe_code)]

//! Reading and writing Unicode text on the Windows clipboard.
//!
//! The whole Win32 conversation (`OpenClipboard`, `CF_UNICODETEXT`, the
//! `GlobalAlloc` block and its ownership hand-off) lives in `sys::clipboard`;
//! this module is the safe surface over it.
//!
//! Text is exchanged as UTF-16, so any Rust `str` survives a round trip,
//! including CJK, combining marks and astral-plane emoji. Newlines are
//! normalised on the way *out* to the clipboard's CRLF convention; see
//! [`set_text`].

use crate::error::Result;
use crate::hwnd::Hwnd;
use crate::sys;

/// Replaces the clipboard contents with `text`, as `CF_UNICODETEXT`.
///
/// `owner` becomes the clipboard owner while the data is written and must be a
/// live window: `SetClipboardData` fails when the clipboard has no owner.
///
/// A lone line feed is written as CRLF, the convention Windows applications
/// expect, while an existing CRLF pair is left alone. [`text`] reads the
/// clipboard back verbatim, so a round trip of text containing `\n` returns the
/// CRLF form.
pub fn set_text(owner: Hwnd, text: &str) -> Result<()> {
    sys::clipboard::write_text(owner, &to_crlf(text))
}

/// Reads the clipboard's Unicode text, or `None` when it holds no
/// `CF_UNICODETEXT` (for example only an image or file list).
///
/// The text is returned exactly as stored; see [`set_text`] on newline
/// normalisation. `owner` is only used to open the clipboard and may be any
/// live window.
pub fn text(owner: Hwnd) -> Result<Option<String>> {
    sys::clipboard::read_text(owner)
}

/// Rewrites every lone line feed as CRLF. An existing `\r\n` is preserved, so
/// applying this to already-normalised text is a no-op.
fn to_crlf(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous = '\0';
    for character in text.chars() {
        if character == '\n' && previous != '\r' {
            out.push('\r');
        }
        out.push(character);
        previous = character;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::to_crlf;

    #[test]
    fn lone_line_feeds_become_crlf() {
        assert_eq!(to_crlf("a\nb"), "a\r\nb");
        assert_eq!(to_crlf("a\n\nb"), "a\r\n\r\nb");
    }

    #[test]
    fn existing_crlf_is_preserved() {
        assert_eq!(to_crlf("a\r\nb"), "a\r\nb");
    }

    #[test]
    fn other_text_is_unchanged() {
        assert_eq!(to_crlf("plain 🎵 text"), "plain 🎵 text");
        assert_eq!(to_crlf(""), "");
    }
}
