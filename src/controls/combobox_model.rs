#![forbid(unsafe_code)]

//! The typed item list behind a [`ComboBox`](super::ComboBox).
//!
//! The native control stores the labels (and reports selection as an index);
//! this maps that index back to the application's value type. It holds no
//! `HWND`, so the mapping can be unit-tested without a window.

/// One combo item: its display `label` and its typed `value`.
pub(crate) struct ComboBoxItem<T> {
    pub(crate) label: String,
    pub(crate) value: T,
}

/// The ordered items a combo box holds, looked up by index or by value.
pub(crate) struct ComboBoxItems<T> {
    items: Vec<ComboBoxItem<T>>,
}

impl<T> ComboBoxItems<T> {
    /// The number of items.
    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there are no items.
    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The display labels, in item order.
    pub(crate) fn labels(&self) -> impl Iterator<Item = &str> + '_ {
        self.items.iter().map(|item| item.label.as_str())
    }

    /// The value at `index`, if it is in range.
    pub(crate) fn value(&self, index: usize) -> Option<&T> {
        self.items.get(index).map(|item| &item.value)
    }

    /// The label at `index`, if it is in range.
    pub(crate) fn label(&self, index: usize) -> Option<&str> {
        self.items.get(index).map(|item| item.label.as_str())
    }
}

impl<T: PartialEq> ComboBoxItems<T> {
    /// The index of `value`, using `PartialEq`.
    pub(crate) fn position(&self, value: &T) -> Option<usize> {
        self.items.iter().position(|item| item.value == *value)
    }
}

impl<T> FromIterator<(String, T)> for ComboBoxItems<T> {
    fn from_iter<I: IntoIterator<Item = (String, T)>>(iter: I) -> Self {
        ComboBoxItems {
            items: iter
                .into_iter()
                .map(|(label, value)| ComboBoxItem { label, value })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ComboBoxItems;

    fn items() -> ComboBoxItems<u32> {
        vec![
            ("Alpha".to_string(), 10u32),
            ("Beta".to_string(), 20),
            ("Gamma".to_string(), 30),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn maps_index_to_value_and_back() {
        let items = items();
        assert_eq!(items.len(), 3);
        assert_eq!(items.value(0), Some(&10));
        assert_eq!(items.value(2), Some(&30));
        assert_eq!(items.value(3), None);
        assert_eq!(items.position(&20), Some(1));
        assert_eq!(items.position(&99), None);
    }

    #[test]
    fn duplicate_values_map_to_the_first() {
        let items: ComboBoxItems<u32> =
            vec![("First".to_string(), 7u32), ("Second".to_string(), 7)]
                .into_iter()
                .collect();
        assert_eq!(items.position(&7), Some(0));
    }

    #[test]
    fn labels_are_in_item_order() {
        let items = items();
        let labels: Vec<&str> = items.labels().collect();
        assert_eq!(labels, ["Alpha", "Beta", "Gamma"]);
        assert_eq!(items.label(1), Some("Beta"));
        assert_eq!(items.label(9), None);
    }
}
