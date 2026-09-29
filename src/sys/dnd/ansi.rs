//! Decoding of ANSI (`fWide == 0`) file lists.

use windows::Win32::Globalization::{CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS, MultiByteToWideChar};

/// Decodes `bytes` from the system ANSI code page (`CP_ACP`), replacing
/// undecodable bytes. Falls back to lossy UTF-8 if the conversion fails.
pub(crate) fn ansi_to_string(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let flags = MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0);
    // SAFETY: a size query (no output buffer) over a valid byte slice.
    let needed = unsafe { MultiByteToWideChar(CP_ACP, flags, bytes, None) };
    if needed <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut wide = vec![0u16; needed as usize];
    // SAFETY: `wide` holds exactly the `needed` units the query reported.
    let written = unsafe { MultiByteToWideChar(CP_ACP, flags, bytes, Some(&mut wide)) };
    if written <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    String::from_utf16_lossy(&wide[..written as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_passes_through() {
        assert_eq!(ansi_to_string(b"dir/a.mp3"), "dir/a.mp3");
        assert_eq!(ansi_to_string(b""), "");
    }

    #[test]
    fn high_bytes_do_not_become_replacement_characters_on_a_single_byte_page() {
        // 0xE9 is 'é' in Windows-1252; on other code pages it still decodes to
        // some character rather than the lossy-UTF-8 U+FFFD.
        assert!(!ansi_to_string(&[b'a', 0xE9]).contains('\u{FFFD}'));
    }
}
