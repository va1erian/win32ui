# win32ui

Native Windows UI for Rust: **small, fast, idiomatic, and properly themed**.
Real Win32 controls, so you get accessibility, IME and system behaviour for
free, with a Rust-shaped API on top and **first-class dark mode**. Originally
extracted from the [emusic](https://github.com/va1erian/emusic) frontend; its
first large consumer is the [esMail](https://github.com/va1erian/esmail)
native frontend.

## Goals

- **Native and small.** Win32 common controls plus GDI and Direct2D, with no
  web runtime, no GPU framework and no C++ toolkit. A window with a few
  controls should cost kilobytes of code, not megabytes.
- **Themed, dark mode included.** Controls theme *themselves*: the application
  picks a `Theme` (or follows the system) and never handles `NM_CUSTOMDRAW`,
  `WM_CTLCOLOR*` or `SetWindowTheme`. Where a native part ignores dark mode,
  win32ui owner-draws it. Only documented APIs are used: no undocumented
  `uxtheme` ordinals.
- **Idiomatic Rust, without a class hierarchy.** See *Architecture*.
- **Composable.** Apps and other crates can write their own widgets
  (`CustomWidget`) that behave exactly like the built-in ones.

## Architecture

There are two layers.

**The platform layer** (exists today) is a safe, honest model of Win32:
`Window` + `WindowHandler` receive a typed `Message` instead of
`(u32, WPARAM, LPARAM)`, with RAII GDI objects, `Paint`/`Canvas`, the message
loop, and `sys/` holding all the `unsafe` code. Use it for custom windows, and as the
escape hatch when the widget layer doesn't cover something.

**The widget layer** is what applications use; the `App`/`Ui` core exists, and
later issues add layout, theming and the remaining controls:

```rust
enum Msg { Search(String), Open(usize), Delete }

struct MailWindow { list: ListView<Row>, search: Edit }

impl App for MailWindow {
    type Msg = Msg;
    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        match msg {
            Msg::Search(q) => self.list.set_model(filter(&q)),
            Msg::Open(i) => { /* ... */ }
            Msg::Delete => { /* ... */ }
        }
    }
}

win32ui::run_app(WindowSpec::new("Mail"), |ui| {
    let search = Edit::single_line(ui).cue("Search").on_change(|t| Some(Msg::Search(t.into())));
    let list = ListView::<Row>::new(ui)
        .column("From", dip(180.0), |r| r.from.as_str())
        .column("Subject", Fill, |r| r.subject.as_str())
        .on_activate(|i| Some(Msg::Open(i)));
    ui.set_layout(column![search, list.fill(1)]);
    MailWindow { list, search }
})
```

`column!` shares its name with a std prelude macro, so the layout macros are
imported explicitly: `use win32ui::{column, row};`.

The design choices, and why:

- **Events are mapped to the app's own `Msg`, not handled in callbacks.** A
  widget is given a small mapping closure when it is built. The resulting
  messages are queued and delivered to `App::update(&mut self, …)`, which is
  **never re-entered**. Win32 calls back into the window procedure
  synchronously (a `SendMessage` inside a handler, a modal menu loop), so
  closure-heavy toolkits end up with `Rc<RefCell<…>>` state that panics under
  re-entry. Here, a message raised while `update` runs is simply delivered
  after it returns. This is the Elm/relm4 shape, applied to retained native
  controls. (#33)
- **Shared behaviour comes from traits, not inheritance.** Every widget holds a
  `Control`; `AsControl` plus a blanket `ControlExt` give every widget
  `set_enabled`, `set_visible`, `focus`, `set_tooltip`, … Capability traits
  (`HasText`, `Themed`) describe what a widget *can do*. There's no base
  class, no `Deref` chain and no downcasting. (#33)
- **No control ids.** Widgets are values you hold. Notifications are routed
  internally by `HWND`. (#33)
- **Layout is a tree the window owns.** `column!`/`row!` with `fill`/`width`,
  re-laid out on resize and DPI change; the moves are batched with
  `DeferWindowPos`, so a resize doesn't flicker. The app never handles
  `WM_SIZE`. (#34, on top of `Dock`/`Stack`)
- **Typed data, not strings and indices.** `ListView<T>` has typed column
  accessors over a `ListModel`, `TreeView<K>` has a keyed lazy model,
  `ComboBox<T>` and `RadioGroup<T>` return values rather than indices. (#19,
  #21, #14, #13)
- **Units are types.** `Dip` for design values, `Px` for device pixels, so DPI
  scaling can't be forgotten or applied twice. (#32)
- **Worker threads talk to the UI through `Proxy<Msg>`**: `Send + Sync`, with
  coalesced wake-ups. (#4)

What we deliberately don't do: a view-diffing engine (native controls are
already retained), and closures that capture shared mutable app state.

### Status

| Area | State |
|---|---|
| Platform layer: windows, typed messages, loop, timers, GDI, re-entrancy-safe dispatch | exists (re-entrancy: #31) |
| `Dock`/`Stack` layout arithmetic | exists |
| Owner-drawn dark `ListView`, `TreeView`, `Toolbar`, `StatusBar`; `Label` | exist (widget-layer API) |
| Widget layer: `App`/`Ui`, `Msg` mapping, `ControlExt`, `run_app` | exists |
| Secondary windows: `Ui::open_window` (non-modal) and `Ui::open_modal` (modal), `WindowHandle` | exists (#45) |
| Layout tree (`column!`/`row!`, `fill`/`width`, relayout on resize/DPI) | exists |
| Theming foundation: tokens, `Themed`, live switching, central `WM_CTLCOLOR*` | #30 |
| Owner-drawn `ProgressBar` (range/value/state/marquee), typed `TaskDialog` | exist (#18) |
| `Edit` (single/multi-line, password), `ComboBox`, and buttons (`Button`,
  `CheckBox`, `RadioGroup`, `GroupBox`) | exist (#12–#14) |
| Menus mapped to `Msg`: menu bar, context popups, owner-drawn dark items | exist (#16) |
| Draggable `split_row!`/`split_col!` layout nodes; themed `ScrollView` (native vertical scrollbar, wheel, `scroll_to`) | exist (#11) |
| Custom widgets: Direct2D paint path (`CustomWidget::renderer`/`paint_d2d`), a built-in vertical scroll host (`Custom::with_vscroll`, `scroll_to`, `Scrolled` event) and rect-scoped repaints (`invalidate_rect`, the dirty rect honoured by both paths) | exist (#64, #83) |
| `Slider`: Direct2D-painted, `f64` values, sub-pixel thumb, mouse capture, coalesced `on_change` + `on_commit` + `on_hover`, eased hover/press/focus, keyboard and wheel, buffered range, vertical, RTL | exists (#46) |
| `FlowText`: wrapped inline runs (normal / weak / link) with per-run clicks, hand cursor and hover underline; rich-text layout with per-range DirectWrite formatting | exists (#48) |
| `tabs!` paged layout node: native `SysTabControl32`, owner-drawn tabs, pages are layout subtrees | exists (#15) |
| Tooltips: `ControlExt::set_tooltip`, region tooltips, toolbar item tooltips, dark owner-draw | exists (#17) |
| Direct2D shapes, clips and transforms (`d2d`, anti-aliased); `ProgressBar` and the owner-drawn shapes (radio, group box, toolbar, tabs, menus, sort arrow) draw with it (GDI fallback) | exists (#22, #77) |
| Mica/Mica Alt/Acrylic backdrop and themed caption (`Backdrop`, `TitleBar`), GDI fallback | exists (#53 phase 1) |
| Extended title bar (`WM_NCCALCSIZE`, `DwmDefWindowProc` hit-test, `caption_inset`, `set_caption_interactive`, top-strip frame extension) | exists (#53 phase 2, fixed by #76) |
| DirectWrite text, gradients, bitmaps, rounded clips, colour emoji | #22 follow-up |
| `GridView<T>`: virtualized tile grid (a `CustomWidget` hosted in a `ScrollView`), typed `GridModel`, single selection, wrap-around keyboard navigation, live tile-size range | exists (#47) |

## Source layout

```
src/
  lib.rs          public API + `prelude`
  error.rs        `Error` / `Result` (thiserror)
  hwnd.rs         `Hwnd`: a Copy handle that never names `windows` types
  geometry.rs     `Point` / `Size` / `Rect` (device pixels)
  units.rs        `Dip` / `Px`: typed design vs device length units
  color.rs        `Color` ↔ Win32 `COLORREF`
  theme.rs        a minimal semantic palette (light/dark)
  message.rs      typed `Message`, `Command`, `Notify`, control events
  window.rs       `WindowClass`, `Window`, `WindowHandler`, style builders
  window/         `Backdrop`/`TitleBar` (backdrop material and themed caption)
  looper.rs       `run()` / `quit()`
  app/            `App`, `Ui`, the per-window message queue, `run_app`
  app/child.rs    secondary windows: `open_window`, `open_modal`, `WindowHandle`
  app/layout/     the layout tree: `column!`/`row!`, `fill`/`width`, relayout
  app/layout/split/   `Split`: `split_row!`/`split_col!` and the divider widget
  app/layout/tabs/    `Tabs`: `tabs!` pages a native tab control's layout subtrees
  gdi/            RAII `Font` / `Brush` / `Pen` / `Bitmap`, `Paint`, `Canvas`
  d2d/            anti-aliased Direct2D: `D2dSurface`/`D2dCanvas` for an `HWND`,
                  `DcCanvas` over an owner-draw `HDC`
  controls/       `ListView`, `TreeView`, `Toolbar`, `StatusBar`, `Label`,
                  `Edit`, `ProgressBar`, `TaskDialog`, `Button`, `CheckBox`,
                  `RadioGroup`, `GroupBox`, `Menu`, `ScrollView`, `FlowText`,
                  `Slider`, `GridView`
  controls/control.rs   `Control`, `AsControl`, `ControlExt`, `HasText`
  controls/registry.rs  routes a control's own notifications back to it
  sys/            ALL `unsafe` lives here; every block has a `// SAFETY:` note
```

## Invariants (please keep them)

- **`unsafe` only in `src/sys/`.** Every other module starts with
  `#![forbid(unsafe_code)]`. `sys` functions are safe to call and contain the
  `unsafe` internally.
- **No `windows` types in the public API.** They may appear as private fields
  (e.g. a `HFONT` inside `gdi::Font`) but callers only see `win32ui` types.
- **Controls own their child `HWND`** and destroy it in `Drop`; they never
  outlive their parent window.
- **Small files.** Aim < 300 lines; split by responsibility.

## How the pieces fit

`Window::create` boxes the caller's `WindowHandler` into a thin
`*mut Box<dyn WindowHandler>` and stashes it in `GWLP_USERDATA` on
`WM_NCCREATE`; the shared `window_proc` (in `sys::dispatch`) decodes each raw
message into a typed `Message` and calls the handler. The handler is shared
(`&self`), so a synchronous message that arrives while the handler is already
on the stack — a `ListView::select()` notification, a `WM_SIZE` from a call
inside the handler, a modal loop — is still delivered rather than dropped;
mutable state lives in `Cell`/`RefCell` fields. The box is reclaimed once, on
`WM_NCDESTROY`; if the window was destroyed from inside its own handler, the
free is deferred until the outermost dispatch for that window returns, so the
handler cannot be freed while a caller still holds `&` to it.

Common controls send their "self-contained" notifications (`LVN_GETDISPINFO`,
`NM_CUSTOMDRAW`, `TVN_ITEMEXPANDING`, …) to their **parent**, not to
themselves. `controls::registry` keeps a thread-local map from child `HWND` to
the Rust state that wants first refusal on those messages; `window_proc` offers
each `WM_NOTIFY` to the registry before decoding it for the application. This is
why the app never sees owner-data/custom-draw plumbing — only `ListViewEvent` /
`TreeViewEvent`s.

The ListView is a real owner-drawn virtual list: `LVS_OWNERDATA` + cell text via
`LVN_GETDISPINFO`, and the whole row painted in `NM_CUSTOMDRAW` (blue selection
highlight, an opt-in zebra background, app-supplied `row_style`/`row_painter`
overrides, column separators). Row height is set with the "1×height image list"
trick (`sys::listview::lv_set_row_height`), the documented way to raise
report-mode row height on an `LVS_OWNERDATA` list — `LVS_OWNERDRAWFIXED` +
`WM_MEASUREITEM` is a different, incompatible style pair that virtual lists
never receive `WM_MEASUREITEM` for. Its header is a separate child control, so
the ListView is subclassed (`sys::control::HeaderSubclass`) to intercept the
header's `NM_CUSTOMDRAW` and paint it dark too.

## Adding a control

Every new control must follow the widget layer: it holds a `Control`, maps its
events to the app's `Msg` through closures given at construction, implements
`Themed`, and its PR includes a light and a dark screenshot. The steps:

1. Add a `sys::control` helper for the raw message(s) you need; keep it safe
   and document each `unsafe` block.
2. Add `controls/<name>.rs`: a struct holding a `Control` (which owns the child
   `HWND`), an inner state implementing `registry::ControlEvents` if it needs
   owner-data/custom-draw, and a `Drop` that unregisters from the registry.
3. Decode application-level notifications into a `…Event` enum and add a
   `Notify::…` variant in `message.rs` + `sys::message::decode_notify`.
4. Map those events to the app's `Msg`: register a `registry::register_app_events`
   mapper (see `controls/listview.rs`) and expose builder methods
   (`on_select`, `on_activate`, …).
5. Implement `Themed`: re-derive colours with `<Control>Theme::from_theme`,
   update native parts via `sys::apply_native_theme`, invalidate, and register
   the child with `theme::register_themed` at creation (unregister in `Drop`).
   See *Theming* below.
6. Re-export it from `lib.rs` (and `prelude`), and exercise it in
   `examples/demo/` + `tests/`.

## Theming

Controls theme themselves; the app never handles `NM_CUSTOMDRAW`,
`WM_CTLCOLOR*` or `SetWindowTheme`.

- **Tokens.** `Theme` is the complete semantic palette (`is_dark`, `background`,
  `surface`, `raised`, `text`/`text_secondary`/`text_disabled`/`text_on_accent`,
  `accent`, `warning`/`danger`, `selection`/`selection_unfocused`, `hover`,
  `pressed`, `border`/`border_focused`, `input_background`,
  `scrollbar`/`scrollbar_track`).
  `Theme::light()`/`dark()` sample Windows 11 Explorer/Settings/WinUI values;
  each field documents its source. Per-control structs
  (`ListViewTheme::from_theme`, `ToolbarTheme::from_theme`,
  `StatusBarTheme::from_theme`, `ProgressBarTheme::from_theme`) are derived,
  overridable views.
- **Live switching.** The theme lives on the window: `Ui::set_theme(theme)` for
  widget apps, `Window::set_theme(theme)` for platform windows. It stores the
  theme for central `WM_CTLCOLOR*` answers, applies the DWM dark title bar
  (`DWMWA_USE_IMMERSIVE_DARK_MODE`), updates the class background (no white
  flashes on resize), re-themes every registered child and repaints once.
  Widgets created through `Ui` adopt its theme automatically and register a
  re-theme callback; dropping a widget unregisters it. Nothing is recreated.
- **Central `WM_CTLCOLOR*`.** The shared window procedure answers
  `WM_CTLCOLOREDIT/STATIC/BTN/LISTBOX/DLG` from the window's theme with cached
  brushes (bounded GDI cache). An app handler that claims the message overrides
  the theme.
- **Native parts.** `sys/theme.rs` picks `DarkMode_Explorer` (scrollable),
  `DarkMode_CFD` (button chrome) or `Explorer` (light) per control kind, plus
  dark scrollbars everywhere. Documented APIs only; where a native part ignores
  dark mode (list header, status bar), win32ui owner-draws it.
- **New controls opt in** by implementing `Themed`, painting only from tokens,
  registering with `theme::register_themed`, and adding a demo toggle state plus
  light/dark screenshots to the PR.
- **Window material.** `WindowSpec::backdrop(Backdrop::Mica | MicaAlt | Acrylic)`
  asks DWM for the system backdrop (`DWMWA_SYSTEMBACKDROP_TYPE`), and
  `WindowSpec::title_bar(TitleBar::Colored)` paints the standard caption from
  theme tokens. Both are best-effort and decided by the documented call's
  result, never by the OS version: when DWM rejects the attribute (Windows 10,
  builds before 22621), in high-contrast mode, or when the user disabled
  transparency effects, the window falls back to the solid `Theme::background`;
  `Ui::backdrop_active()` reports which path was taken.
- **Extended title bar.** `WindowSpec::title_bar(TitleBar::Extended)` removes
  the standard caption (`WM_NCCALCSIZE`) while keeping the native resize
  borders, and routes `WM_NCHITTEST` through `DwmDefWindowProc` first so the
  min/max/close buttons — and with them Windows 11 snap layouts — keep working.
  A restored window's client starts at its top edge, so the caption strip is
  client area (the top resize band is hit-tested by the crate); a maximized
  window is inset by the frame it overhangs the monitor with. The frame is
  extended over that strip only (`DwmExtendFrameIntoClientArea` with a top
  margin), so DWM draws the caption buttons there and a backdrop material shows
  through it while the rest of the client stays an ordinary opaque surface. The
  strip is erased to black (DWM's "glass" colour). The strip's caption colour is
  set to none when the material is active — otherwise "show accent colour on
  title bars" paints the accent over it — and to `Theme::background` when not.
  The menu bar stays non-client, drawn by the system just below the strip; the
  window's erase leaves that band alone, and the demo reserves strip and menu
  with `Ui::title_bar_height()` so content never sits under either.
  The free strip drags, widgets marked with
  `ControlExt::set_caption_interactive` accept clicks, and
  `Ui::caption_inset()` reserves the button area. Content painted over the
  material still needs Direct2D alpha (GDI text over the glass writes zero
  alpha), so a widget in the strip — the `title_bar` layout item — remains the
  follow-up.

## Menus

Menus are data mapped to the app's `Msg`, like every other widget:

```rust
let menu = Menu::new()
    .item("&Reply", Shortcut::ctrl(Key::R), || Msg::Reply)
    .separator()
    .checked_item("&Wrap", None, true, || Msg::ToggleWrap)
    .disabled_item("&Archive", None, || Msg::Archive);
ui.set_menu_bar(menu);
// then, from a widget event: `ui.popup(&context, ui.cursor_position())`
```

`Ui::set_menu_bar` installs the bar and registers each enabled item's
`Shortcut` as an accelerator, so the menu and the keyboard always agree.
`Ui::popup` runs `TrackPopupMenuEx(TPM_RETURNCMD)` and queues the chosen
item's message; as with every widget event, `App::update` is never re-entered.
On a dark theme the items are owner-drawn (`MF_OWNERDRAW`, painted from theme
tokens on `WM_MEASUREITEM`/`WM_DRAWITEM`); on the light theme the native menu
is used, and switching the theme rebuilds the bar in place. Only documented
APIs are used — no `uxtheme` ordinals.

## Tabs

`tabs!` is a paged layout node: its pages are layout subtrees, shown and hidden
automatically, so the app never places them or handles a resize for them.

```rust
let tabs = tabs![
    ("General", column![general_label.fill(1)]),
    ("Accounts", column![accounts_label.fill(1)]),
]
.on_change(|index| Some(Msg::Tab(index)));
ui.set_layout(column![tabs]);
```

The node owns a native `SysTabControl32`; the selected page is laid out into the
control's display area (`TCM_ADJUSTRECT`), and every other page is hidden (so it
takes no space and cannot receive focus). The native tabs ignore dark mode, so
the control is `TCS_OWNERDRAWFIXED` and each tab is painted from theme tokens on
`WM_DRAWITEM` — including hover, selected and focus states — with a small
subclass tracking the hot tab and `Ctrl+Tab`.

## Trees

`TreeView<K, M>` is keyed and lazily loaded. The app implements `TreeModel`
for a key type `K`; only the roots are
read up front, and a branch's `children` are fetched the first time it is
expanded. `refresh` then diffs the materialized tree against the model *by key*
(a pure function, unit-tested in `controls/treeview/diff.rs`), so a changing
unread count updates in place while expansion, selection and scroll survive:

```rust
let tree = TreeView::new(ui, folders)?
    .images(folder_icons)                       // RAII ImageList
    .style(|id| NodeStyle::new().badge(unread(*id)).icon(icon(*id)))
    .on_select(|id| Some(Msg::OpenFolder(*id)))
    .on_toggle(|id, expanded| Some(Msg::Folded(*id, expanded)));

tree.refresh();                                 // diff by key; keeps state
tree.select(&inbox); tree.expand(&archive, true);
```

`NodeStyle` is read when nodes are inserted and on `refresh` (never in the
paint path), and the widget caches it per node; selection is owner-drawn from
theme tokens, and the native tree draws the expand glyphs and the icons from
the `ImageList` you hand it. `select` and `expand` are programmatic: they emit
no `on_select`/`on_toggle` of their own beyond the one selection message, so
calling them from those handlers cannot loop. (The issue's
`TreeView::<K>::new(ui, …)` shorthand is `TreeView<K, M>` here, matching
`ListView<T, M>`: `M` is the app's message type.)

## Tooltips

Every widget gets a tooltip through the shared `ControlExt` capability, and a
custom widget can declare one region of itself:

```rust
progress.set_tooltip("Scan progress");                 // any widget
ToolbarItem::new("Refresh").tooltip("Refresh").shortcut(Shortcut::ctrl(Key::R));
// inside CustomWidget::input:
cx.set_tooltip_region(rect, "Left half");
```

One `tooltips_class32` window (`TTS_ALWAYSTIP | TTS_NOPREFIX`) is created lazily
per top-level window and owned by it; every widget's tooltip is a *tool* on that
one window, so widgets never each create their own. A `ToolbarItem` with a
`shortcut` shows the shortcut's display text after its tooltip text — the same
string its menu item shows. On a dark theme the tooltip is owner-drawn through
the documented `NM_CUSTOMDRAW` notification: `SetWindowTheme("DarkMode_Explorer")`
alone does not darken a tooltip, and `TTM_SETTIPBKCOLOR`/`TTM_SETTIPTEXTCOLOR`
are ignored while visual styles are on. The tooltip is given the same
DPI-scaled font comctl32 sizes it with through `WM_SETFONT` (re-applied after
`SetWindowTheme`, which resets it, and on `WM_DPICHANGED`), so the window is
never sized for a narrower font than the one the text is painted with; a
DPI-scaled `TTM_SETMAXTIPWIDTH` makes a long tip wrap instead of clip. The
owner-draw reads the shown text from our own tool list (the tool whose area
contains the cursor) rather than sending a `TTM_*` query back to the control
while it is blocked in `SendMessage`; it also skips an empty text, because
`DrawTextW` faults on an empty buffer.

## Dark theming notes

- Rows/header are painted by us, so their colours come from `ListViewTheme`.
- The scroll bar is themed with `SetWindowTheme(hwnd, "DarkMode_Explorer",
  null)` (documented API, works on Win10/11). The native header does **not**
  honour it, which is why the header is owner-drawn.
- The native status bar exposes no text colour, so `StatusBar` is owner-drawn
  too.
- DPI: `win32ui::init()` opts into per-monitor-v2 awareness; design values are
  written as `Dip` and converted once with `Dip::to_px(dpi)`. The example
  binaries embed a Common Controls v6 + DPI manifest (`win32ui.rc` /
  `win32ui.manifest`, via `build.rs`).

## Running

```
cargo run --example demo
```

Set `WIN32UI_DEMO_AUTOCLOSE_MS=4000` to have the demo quit itself (used for
headless smoke runs). The demo's data deliberately includes CJK, astral-plane
emoji, combining marks and RTL text; `tests/` and `src/controls/listview.rs`
unit tests assert those round-trip through the UTF-16 owner-data path.
