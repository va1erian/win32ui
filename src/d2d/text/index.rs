//! Conversions between UTF-8 byte offsets (the public unit) and the UTF-16
//! code units DirectWrite works in.

/// The UTF-16 offset of byte offset `byte` in `text`, snapping down to a
/// character boundary and clamping to the end.
pub(super) fn to_utf16(text: &str, byte: usize) -> usize {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    text[..byte].encode_utf16().count()
}

/// The byte offset reached by moving `units` UTF-16 code units on from byte
/// offset `from`, clamping to the end.
pub(super) fn advance(text: &str, from: usize, units: usize) -> usize {
    let mut remaining = units;
    let mut byte = from;
    for ch in text[from..].chars() {
        if remaining == 0 {
            break;
        }
        remaining = remaining.saturating_sub(ch.len_utf16());
        byte += ch.len_utf8();
    }
    byte
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_offsets_are_equal() {
        assert_eq!(to_utf16("hello", 3), 3);
        assert_eq!(advance("hello", 1, 3), 4);
    }

    #[test]
    fn astral_characters_take_two_units() {
        let text = "a👋b";
        assert_eq!(to_utf16(text, 1), 1);
        assert_eq!(to_utf16(text, 5), 3);
        assert_eq!(advance(text, 1, 2), 5);
    }

    #[test]
    fn mid_character_offsets_snap_down() {
        assert_eq!(to_utf16("é", 1), 0);
        assert_eq!(to_utf16("é", 99), 1);
    }
}
