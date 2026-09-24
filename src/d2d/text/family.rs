//! CSS-style font family lists.

/// Splits a family list (`"Arial, 'Segoe UI', sans-serif"`) into candidate
/// family names in order, mapping the CSS generic families to the Windows
/// fonts that stand in for them.
pub(super) fn candidates(list: &str) -> Vec<String> {
    list.split(',')
        .map(|entry| entry.trim().trim_matches(['"', '\'']).trim())
        .filter(|entry| !entry.is_empty())
        .map(generic)
        .collect()
}

/// The installed font behind a CSS generic family name; any other name is
/// returned unchanged. `system-ui` is the face the OS uses for its own UI, the
/// same one the GDI controls get from `Font::system_ui`.
fn generic(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "serif" => "Cambria".to_owned(),
        "sans-serif" => "Segoe UI".to_owned(),
        "system-ui" => crate::gdi::system_ui_family(),
        "monospace" | "ui-monospace" => "Consolas".to_owned(),
        _ => name.to_owned(),
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
    fn system_ui_is_the_gdi_ui_face() {
        assert_eq!(candidates("system-ui"), [crate::gdi::system_ui_family()]);
    }

    #[test]
    fn an_empty_list_has_no_candidates() {
        assert!(candidates(" , ").is_empty());
    }
}
