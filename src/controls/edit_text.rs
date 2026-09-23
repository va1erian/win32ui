#![forbid(unsafe_code)]

//! `CRLF`/`LF` conversion between Rust strings and a native multi-line
//! `EDIT`, which stores `\r\n` line endings.
//!
//! Single-line edits never contain a line break, so these are no-ops for them.

/// Normalises Rust text (`LF`) into the `CRLF` line endings a native edit
/// stores, first folding any existing `CRLF`/lone `CR` so a round trip is
/// lossless.
pub(crate) fn to_native(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push_str("\r\n");
        } else if c == '\n' {
            out.push_str("\r\n");
        } else {
            out.push(c);
        }
    }
    out
}

/// Normalises a native edit's `CRLF` text back to Rust's `LF`.
pub(crate) fn from_native(text: &str) -> String {
    text.replace("\r\n", "\n")
}

#[cfg(test)]
mod tests {
    use super::{from_native, to_native};

    #[test]
    fn lf_becomes_crlf_for_native_storage() {
        assert_eq!(to_native("a\nb"), "a\r\nb");
        assert_eq!(to_native("a\r\nb"), "a\r\nb");
        assert_eq!(to_native("a\rb"), "a\r\nb");
    }

    #[test]
    fn crlf_comes_back_as_lf() {
        assert_eq!(from_native("a\r\nb"), "a\nb");
        assert_eq!(from_native("a\nb"), "a\nb");
    }

    #[test]
    fn round_trip_is_lossless() {
        for text in ["", "plain", "a\nb\nc", "日本語\n🎵", "e\u{301}\n"] {
            assert_eq!(from_native(&to_native(text)), text);
        }
    }
}
