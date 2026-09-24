//! The demo's Form tab: a scrollable settings form of standard controls.
//!
//! It shows the [`Panel`] + [`ScrollView`] pair from #119: the controls are
//! parented to the panel (created through `panel.ui`), positioned by the
//! panel's own layout, and the whole form scrolls in the viewport. It also shows
//! the [`ColorPicker`] from #121 next to the other fields.

use win32ui::column;
use win32ui::prelude::*;

use super::Msg;

/// What the form's fields tell the app.
pub(super) enum FormMsg {
    /// The display-name field changed.
    Name(String),
    /// The subscribe check box toggled.
    Subscribe(bool),
    /// A colour was picked.
    Accent(Color),
    /// The form was submitted.
    Save,
}

/// The tab's widgets. The fields are held here so their windows stay alive.
pub(super) struct Form {
    view: ScrollView,
    panel: Panel,
    header: Label,
    name_caption: Label,
    name: Edit<Msg>,
    subscribe: CheckBox<Msg>,
    accent_caption: Label,
    accent: ColorPicker<Msg>,
    save: Button<Msg>,
}

impl Form {
    pub(super) fn build(ui: &mut Ui<Msg>) -> Form {
        let view = ScrollView::new(ui).expect("scroll");
        let panel = Panel::new(ui).expect("panel");
        let theme = ui.theme();
        // The fields are created through the panel's scoped `Ui`, so they are
        // parented to the panel and scroll with it.
        let mut form = panel.ui(ui);
        let header = Label::new(&mut form, Rect::default(), "Account").expect("header");
        let name_caption =
            Label::new(&mut form, Rect::default(), "Display name").expect("name label");
        let name = Edit::single_line(&mut form)
            .expect("name")
            .cue("Your name")
            .on_change(|text| Some(Msg::Form(FormMsg::Name(text.to_owned()))));
        let subscribe = CheckBox::new(&mut form, "Subscribe to the newsletter")
            .expect("subscribe")
            .checked(true)
            .on_toggle(|checked| Some(Msg::Form(FormMsg::Subscribe(checked))));
        let accent_caption =
            Label::new(&mut form, Rect::default(), "Accent colour").expect("accent label");
        let accent = ColorPicker::new(&mut form, theme.accent)
            .expect("accent")
            .on_change(|color| Some(Msg::Form(FormMsg::Accent(color))));
        let save = Button::new(&mut form, "Save")
            .expect("save")
            .on_click(|| Some(Msg::Form(FormMsg::Save)));

        panel.set_layout(
            column![
                header.height(dip(24.0)),
                name_caption.height(dip(20.0)),
                name.height(dip(26.0)),
                subscribe.height(dip(24.0)),
                accent_caption.height(dip(20.0)),
                accent.height(dip(26.0)),
                save.height(dip(28.0)),
            ]
            .spacing(dip(10.0))
            .margins(Insets::all(dip(12.0))),
        );
        view.set_content(&panel);
        // The form is taller than the viewport, so the viewport scrolls it.
        view.set_content_height(dip(420.0).to_px(ui.dpi()));

        Form {
            view,
            panel,
            header,
            name_caption,
            name,
            subscribe,
            accent_caption,
            accent,
            save,
        }
    }

    pub(super) fn page(&self) -> Layout {
        column![self.view.fill(1)]
    }

    /// Handles the tab's messages. Returns whether `msg` was one.
    pub(super) fn update(&mut self, msg: &Msg, status: &dyn super::StatusWriter) -> bool {
        let Msg::Form(msg) = msg else {
            return false;
        };
        match msg {
            FormMsg::Name(name) => status.set_text(0, &format!("Name: {name}")),
            FormMsg::Subscribe(checked) => status.set_text(
                0,
                if *checked {
                    "Subscribed"
                } else {
                    "Unsubscribed"
                },
            ),
            FormMsg::Accent(color) => status.set_text(
                0,
                &format!("Accent #{:02X}{:02X}{:02X}", color.r, color.g, color.b),
            ),
            FormMsg::Save => status.set_text(0, "Form saved"),
        }
        // Keep the fields used (they are widgets held for their windows).
        let _ = (
            &self.panel,
            &self.header,
            &self.name_caption,
            &self.name,
            &self.subscribe,
            &self.accent_caption,
            &self.accent,
            &self.save,
        );
        true
    }
}
