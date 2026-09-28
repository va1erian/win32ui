#![forbid(unsafe_code)]

//! Parsing of the `DROPFILES` block behind `CF_HDROP` (an Explorer file drop).

use std::path::PathBuf;

/// Size of the `DROPFILES` header: `pFiles: u32`, `pt: POINT` (8 bytes),
/// `fNC: BOOL` and `fWide: BOOL` (`shlobj_core.h`).
const HEADER: usize = 20;

/// The file paths in a raw `DROPFILES` block.
///
/// The list is a run of nul-terminated strings ended by an empty one, in UTF-16
/// when the header's `fWide` flag is set (always, for Explorer) and in the
/// ANSI code page otherwise (decoded here as lossy UTF-8). A malformed block
/// yields the paths that could be read.
pub(crate) fn parse(block: &[u8]) -> Vec<PathBuf> {
    let (Some(offset), Some(wide)) = (read_u32(block, 0), read_u32(block, 16)) else {
        return Vec::new();
    };
    let offset = (offset as usize).max(HEADER);
    let Some(list) = block.get(offset..) else {
        return Vec::new();
    };
    if wide != 0 {
        let units: Vec<u16> = list
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        units
            .split(|unit| *unit == 0)
            .take_while(|path| !path.is_empty())
            .map(|path| PathBuf::from(String::from_utf16_lossy(path)))
            .collect()
    } else {
        list.split(|byte| *byte == 0)
            .take_while(|path| !path.is_empty())
            .map(|path| PathBuf::from(String::from_utf8_lossy(path).into_owned()))
            .collect()
    }
}

fn read_u32(block: &[u8], at: usize) -> Option<u32> {
    let bytes = block.get(at..at + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(wide: bool, paths: &[&str]) -> Vec<u8> {
        let mut out = vec![0u8; HEADER];
        out[0..4].copy_from_slice(&(HEADER as u32).to_le_bytes());
        out[16..20].copy_from_slice(&u32::from(wide).to_le_bytes());
        for path in paths {
            if wide {
                for unit in path.encode_utf16() {
                    out.extend_from_slice(&unit.to_le_bytes());
                }
                out.extend_from_slice(&[0, 0]);
            } else {
                out.extend_from_slice(path.as_bytes());
                out.push(0);
            }
        }
        out.extend_from_slice(if wide { &[0, 0] } else { &[0] });
        out
    }

    #[test]
    fn reads_wide_paths() {
        let bytes = block(true, &["C:\\music\\a.flac", "C:\\music\\é ü.mp3"]);
        assert_eq!(
            parse(&bytes),
            vec![
                PathBuf::from("C:\\music\\a.flac"),
                PathBuf::from("C:\\music\\é ü.mp3")
            ]
        );
    }

    #[test]
    fn reads_ansi_paths() {
        let bytes = block(false, &["C:\\a.txt"]);
        assert_eq!(parse(&bytes), vec![PathBuf::from("C:\\a.txt")]);
    }

    #[test]
    fn tolerates_garbage() {
        assert!(parse(&[]).is_empty());
        assert!(parse(&[1, 2, 3]).is_empty());
        let mut truncated = block(true, &["C:\\a"]);
        truncated.truncate(HEADER + 3);
        assert!(parse(&truncated).len() <= 1);
        let mut past_end = block(true, &["C:\\a"]);
        past_end[0..4].copy_from_slice(&9999u32.to_le_bytes());
        assert!(parse(&past_end).is_empty());
    }
}
