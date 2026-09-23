#![forbid(unsafe_code)]

//! A modal task dialog returning typed choices.
//!
//! Choices are the application's own values: each button carries one, and
//! [`TaskDialog::show`] returns the chosen value (plus whether the verification
//! checkbox was ticked). The dialog is modal to the [`Ui`] window; like any
//! native modal loop it is safe to call from [`App::update`](crate::App::update),
//! which is never re-entered.

use crate::app::Ui;
use crate::error::{Error, Result};
use crate::sys;
use crate::sys::taskdialog::{Outcome, Request};

/// A standard task-dialog icon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TaskDialogIcon {
    /// No icon.
    #[default]
    None,
    /// Informational (a blue "i").
    Information,
    /// Warning (a yellow triangle).
    Warning,
    /// Error (a red circle).
    Error,
    /// Shield, for actions that need elevation.
    Shield,
}

/// A modal task dialog with typed button choices.
///
/// ```no_run
/// use win32ui::prelude::*;
///
/// #[derive(Clone, PartialEq)]
/// enum Choice {
///     Delete,
///     Cancel,
/// }
///
/// fn confirm(ui: &Ui<()>) -> Result<()> {
///     let (choice, _dont_ask) = TaskDialog::new("Delete 3 messages?")
///         .content("They will be moved to Trash.")
///         .buttons([("Delete", Choice::Delete), ("Cancel", Choice::Cancel)])
///         .default(Choice::Cancel)
///         .icon(TaskDialogIcon::Warning)
///         .verification("Don't ask again")
///         .show(ui)?;
///     let _ = choice;
///     Ok(())
/// }
/// ```
pub struct TaskDialog<C> {
    title: String,
    content: Option<String>,
    buttons: Vec<(String, C)>,
    default: Option<C>,
    icon: TaskDialogIcon,
    verification: Option<String>,
}

impl<C: Clone> TaskDialog<C> {
    /// A dialog whose bold main instruction is `title`.
    pub fn new(title: impl Into<String>) -> TaskDialog<C> {
        TaskDialog {
            title: title.into(),
            content: None,
            buttons: Vec::new(),
            default: None,
            icon: TaskDialogIcon::None,
            verification: None,
        }
    }

    /// Adds the body text shown under the main instruction.
    pub fn content(mut self, content: impl Into<String>) -> TaskDialog<C> {
        self.content = Some(content.into());
        self
    }

    /// Sets the buttons, in order, each paired with the choice it returns.
    ///
    /// Button ids are assigned `1..=len`; the second button therefore doubles
    /// as the dialog's cancel button (`IDCANCEL`), which is what `Esc` and the
    /// close box activate.
    pub fn buttons<I, S>(mut self, buttons: I) -> TaskDialog<C>
    where
        I: IntoIterator<Item = (S, C)>,
        S: Into<String>,
    {
        self.buttons = buttons
            .into_iter()
            .map(|(label, choice)| (label.into(), choice))
            .collect();
        self
    }

    /// Sets which choice is activated by `Enter`. A choice not among the
    /// buttons is ignored.
    pub fn default(mut self, choice: C) -> TaskDialog<C>
    where
        C: PartialEq,
    {
        self.default = Some(choice);
        self
    }

    /// Sets the standard icon.
    pub fn icon(mut self, icon: TaskDialogIcon) -> TaskDialog<C> {
        self.icon = icon;
        self
    }

    /// Adds a verification checkbox (e.g. "Don't ask again"). Its state is the
    /// second element of [`TaskDialog::show`]'s result.
    pub fn verification(mut self, label: impl Into<String>) -> TaskDialog<C> {
        self.verification = Some(label.into());
        self
    }

    /// Shows the dialog, modal to `ui`, and returns the chosen value together
    /// with the verification checkbox's state.
    ///
    /// Dismissing the dialog (the close box, or `Esc` when no button carries
    /// the cancel id) returns the default choice. Returns
    /// [`Error::TaskDialogUnavailable`] when Common Controls v6 is not loaded.
    pub fn show<M: 'static>(&self, ui: &Ui<M>) -> Result<(C, bool)>
    where
        C: PartialEq,
    {
        self.validate()?;
        let default_index = self
            .default
            .as_ref()
            .and_then(|choice| self.buttons.iter().position(|(_, value)| value == choice))
            .unwrap_or(0);
        let labels: Vec<&str> = self
            .buttons
            .iter()
            .map(|(label, _)| label.as_str())
            .collect();
        let request = Request {
            owner: ui.hwnd(),
            title: &self.title,
            content: self.content.as_deref(),
            buttons: labels,
            default_index,
            icon: self.icon,
            verification: self.verification.as_deref(),
        };
        let Outcome { button_id, checked } = sys::taskdialog::show(&request)?;
        let index = match usize::try_from(button_id) {
            Ok(id) if (1..=self.buttons.len()).contains(&id) => id - 1,
            _ => default_index,
        };
        Ok((self.buttons[index].1.clone(), checked))
    }

    /// Checks what must hold before touching Win32.
    fn validate(&self) -> Result<()> {
        if self.buttons.is_empty() {
            Err(Error::TaskDialog("at least one button is required".into()))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Choice {
        Delete,
        Cancel,
    }

    fn dialog() -> TaskDialog<Choice> {
        TaskDialog::new("Delete 3 messages?")
            .content("They will be moved to Trash.")
            .buttons([("Delete", Choice::Delete), ("Cancel", Choice::Cancel)])
            .default(Choice::Cancel)
            .icon(TaskDialogIcon::Warning)
            .verification("Don't ask again")
    }

    #[test]
    fn builder_records_its_fields() {
        let dialog = dialog();
        assert_eq!(dialog.title, "Delete 3 messages?");
        assert_eq!(
            dialog.content.as_deref(),
            Some("They will be moved to Trash.")
        );
        assert_eq!(
            dialog.buttons,
            vec![
                ("Delete".to_string(), Choice::Delete),
                ("Cancel".to_string(), Choice::Cancel),
            ]
        );
        assert_eq!(dialog.default, Some(Choice::Cancel));
        assert_eq!(dialog.icon, TaskDialogIcon::Warning);
        assert_eq!(dialog.verification.as_deref(), Some("Don't ask again"));
    }

    #[test]
    fn show_rejects_an_empty_button_list() {
        let dialog: TaskDialog<Choice> = TaskDialog::new("hi");
        assert!(matches!(dialog.validate(), Err(Error::TaskDialog(_))));
    }
}
