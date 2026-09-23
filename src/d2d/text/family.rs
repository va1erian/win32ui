//! CSS-style font family lists.

/// Splits a family list (`"Arial, 'Segoe UI', sans-serif"`) into candidate
/// family names in order, mapping the CSS generic families to the Windows
/// fonts that stand in for them.
pub(super) fn candidates(list: &str) -> Vec<String> {
    list.split(',')
        .map(|entry| entry.trim().trim_matches(['"', '\'']).trim())
        .filter(|entry| !entry.is_empty())
        .map(|entry| generic(entry).unwrap_or(entry).to_owned())
        .collect()
}

/// The installed font behind a CSS generic family name, if `name` is one.
fn generic(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "serif" => Some("Cambria"),
        "sans-serif" | "system-ui" => Some("Segoe UI"),
        "monospace" | "ui-monospace" => Some("Consolas"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_trims_and_unquotes() {
        assert_eq!(
            candidates(" Arial ,'Segoe UI', \"Arial Black\" ,, "),
            ["Arial", "Segoe UI", "Arial Black"]
        );
    }

    #[test]
    fn maps_generic_families() {
        assert_eq!(
            candidates("serif, Sans-Serif, monospace"),
            ["Cambria", "Segoe UI", "Consolas"]
        );
    }

    #[test]
    fn an_empty_list_has_no_candidates() {
        assert!(candidates(" , ").is_empty());
    }
}
