//! The demo's modal task dialog and the typed choice it returns.

use win32ui::prelude::*;

use super::Msg;

/// The typed choices the demo's task dialog can return.
#[derive(Clone, PartialEq, Eq)]
enum DialogChoice {
    Delete,
    Cancel,
}

/// Handles the demo's task-dialog message. Returns whether `msg` was it.
pub(super) fn update(msg: &Msg, ui: &mut Ui<Msg>, status: &dyn super::StatusWriter) -> bool {
    match msg {
        Msg::Clear => match TaskDialog::new("Delete 3 messages?")
            .content("They will be moved to Trash.")
            .buttons([
                ("Delete", DialogChoice::Delete),
                ("Cancel", DialogChoice::Cancel),
            ])
            .default(DialogChoice::Cancel)
            .icon(TaskDialogIcon::Warning)
            .verification("Don't ask again")
            .show(ui)
        {
            Ok((DialogChoice::Delete, dont_ask)) => status.set_text(
                0,
                if dont_ask {
                    "Deleted 3 messages (and won't ask again)"
                } else {
                    "Deleted 3 messages"
                },
            ),
            Ok((DialogChoice::Cancel, _)) => status.set_text(0, "Delete cancelled"),
            Err(error) => status.set_text(0, &format!("Task dialog unavailable: {error}")),
        },
        _ => return false,
    }
    true
}
