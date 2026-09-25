#![forbid(unsafe_code)]

//! The accessibility tree model: [`Node`], [`Role`], [`Action`].

use crate::geometry::Rect;

/// What kind of thing a [`Node`] is, as a screen reader announces it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// A push button.
    Button,
    /// A check box (toggles on and off).
    CheckBox,
    /// One option of a radio group.
    RadioButton,
    /// A slider or scrub bar (a range value).
    Slider,
    /// A progress indicator.
    ProgressBar,
    /// Static text.
    Text,
    /// An editable text field.
    Edit,
    /// A drop-down list; its children are the choices.
    ComboBox,
    /// A list of items.
    List,
    /// One item in a list.
    ListItem,
    /// A tab strip.
    Tab,
    /// One tab of a strip.
    TabItem,
    /// A tool bar.
    ToolBar,
    /// A menu bar.
    MenuBar,
    /// A menu item; one with children is a submenu.
    MenuItem,
    /// A status bar.
    StatusBar,
    /// A grouping of related nodes.
    Group,
    /// A picture.
    Image,
    /// A document-like view.
    Document,
    /// A separator or splitter.
    Separator,
    /// A top-level window. On a window's root node the name and type stay
    /// with Windows; only the children are contributed.
    Window,
    /// Anything else; announced as a generic custom control.
    Custom,
}

/// A numeric value with bounds, for sliders and progress indicators.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeValue {
    /// The smallest value.
    pub min: f64,
    /// The largest value.
    pub max: f64,
    /// The current value.
    pub value: f64,
    /// The keyboard step (small change).
    pub step: f64,
    /// Whether a client may set the value.
    pub settable: bool,
}

/// What a client asks a widget to do to a node.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Activate (click) the node.
    Invoke,
    /// Flip a checkable node.
    Toggle,
    /// Select the node (a list item, a tab, a radio option).
    Select,
    /// Move a range node to this value.
    SetRange(f64),
    /// Replace an editable node's text.
    SetText(String),
    /// Give the node the keyboard focus.
    Focus,
}

/// One element of a widget's accessibility tree.
///
/// Build it with [`Node::new`] and the chained setters; a container adds its
/// children with [`Node::child`].
#[derive(Clone, Debug)]
pub struct Node {
    /// What the node is.
    pub role: Role,
    /// The accessible name (what a screen reader reads).
    pub name: String,
    /// A textual value (an edit's text, a slider readout), if any.
    pub value: Option<String>,
    /// A numeric range, for sliders and progress indicators.
    pub range: Option<RangeValue>,
    /// The checked state of a checkable node.
    pub checked: Option<bool>,
    /// Whether a selectable node is selected.
    pub selected: Option<bool>,
    /// Whether the node can be activated with [`Action::Invoke`].
    pub invokable: bool,
    /// Whether the node is enabled.
    pub enabled: bool,
    /// Whether the node can take the keyboard focus.
    pub focusable: bool,
    /// Whether the node has the keyboard focus.
    pub focused: bool,
    /// A stable id for automated clients (unique among the widget's nodes).
    pub id: Option<String>,
    /// Longer help text (a tooltip).
    pub help: Option<String>,
    /// The node bounds in the widget client pixels; `None` means the parent
    /// bounds (or the whole widget, for the root).
    pub bounds: Option<Rect>,
    /// The node children, in reading order.
    pub children: Vec<Node>,
}

impl Node {
    /// A node with a role and an accessible name; enabled, not focusable.
    pub fn new(role: Role, name: impl Into<String>) -> Node {
        Node {
            role,
            name: name.into(),
            value: None,
            range: None,
            checked: None,
            selected: None,
            invokable: false,
            enabled: true,
            focusable: false,
            focused: false,
            id: None,
            help: None,
            bounds: None,
            children: Vec::new(),
        }
    }

    /// Sets the textual value.
    pub fn value(mut self, value: impl Into<String>) -> Node {
        self.value = Some(value.into());
        self
    }

    /// Makes the node a settable range.
    pub fn range(mut self, min: f64, max: f64, value: f64, step: f64) -> Node {
        self.range = Some(RangeValue {
            min,
            max,
            value,
            step,
            settable: true,
        });
        self
    }

    /// Makes the node a read-only range (a progress indicator).
    pub fn read_only_range(mut self, min: f64, max: f64, value: f64) -> Node {
        self.range = Some(RangeValue {
            min,
            max,
            value,
            step: 0.0,
            settable: false,
        });
        self
    }

    /// Marks the node checkable and sets its state.
    pub fn checked(mut self, checked: bool) -> Node {
        self.checked = Some(checked);
        self
    }

    /// Marks the node selectable and sets its state.
    pub fn selected(mut self, selected: bool) -> Node {
        self.selected = Some(selected);
        self
    }

    /// Lets a client activate the node with [`Action::Invoke`].
    pub fn invokable(mut self) -> Node {
        self.invokable = true;
        self
    }

    /// Sets whether the node is enabled.
    pub fn enabled(mut self, enabled: bool) -> Node {
        self.enabled = enabled;
        self
    }

    /// Marks the node keyboard-focusable, with its current focus state.
    pub fn focusable(mut self, focused: bool) -> Node {
        self.focusable = true;
        self.focused = focused;
        self
    }

    /// Sets the stable automation id.
    pub fn id(mut self, id: impl Into<String>) -> Node {
        self.id = Some(id.into());
        self
    }

    /// Sets the help text.
    pub fn help(mut self, help: impl Into<String>) -> Node {
        self.help = Some(help.into());
        self
    }

    /// Sets the bounds in the widget client pixels.
    pub fn bounds(mut self, bounds: Rect) -> Node {
        self.bounds = Some(bounds);
        self
    }

    /// Appends a child.
    pub fn child(mut self, child: Node) -> Node {
        self.children.push(child);
        self
    }

    /// Appends several children.
    pub fn children(mut self, children: impl IntoIterator<Item = Node>) -> Node {
        self.children.extend(children);
        self
    }

    /// The node at `path` (an index chain from this node), if it exists.
    pub fn at(&self, path: &[usize]) -> Option<&Node> {
        path.iter()
            .try_fold(self, |node, index| node.children.get(*index))
    }
}
