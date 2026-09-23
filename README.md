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

**The widget layer** (being built; see the issues linked below) is what
applications use:

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
  re-laid out on resize and DPI change. The app never handles `WM_SIZE`.
  (#34, on top of `Dock`/`Stack`)
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
| Owner-drawn dark `ListView`, `TreeView`, `Toolbar`, `StatusBar`; `Label` | exist (platform-style API; ported to the widget layer in #33) |
| Widget layer: `App`/`Ui`, `Msg` mapping, `ControlExt` | #33 |
| Theming foundation: tokens, `Themed`, live switching, central `WM_CTLCOLOR*` | #30 |
| Edit, buttons, ComboBox, tabs, menus, tooltips, progress/task dialog, split/scroll | #11–#18 |
| Direct2D/DirectWrite (anti-aliasing, colour emoji) | #22 |

## Source layout

```
src/
  lib.rs          public API + `prelude`
  error.rs        `Error` / `Result` (thiserror)
  hwnd.rs         `Hwnd`: a Copy handle that never names `windows` types
  geometry.rs     `Point` / `Size` / `Rect`, with split helpers for layout
  color.rs        `Color` ↔ Win32 `COLORREF`
  theme.rs        a minimal semantic palette (light/dark)
  message.rs      typed `Message`, `Command`, `Notify`, control events
  window.rs       `WindowClass`, `Window`, `WindowHandler`, style builders
  looper.rs       `run()` / `quit()`
  gdi/            RAII `Font` / `Brush` / `Pen` / `Bitmap`, `Paint`, `Canvas`
  controls/       `ListView`, `TreeView`, `Toolbar`, `StatusBar`, `Label`
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
`LVN_GETDISPINFO`, and the whole row painted in `NM_CUSTOMDRAW` (zebra, blue
selection/playing highlight, column separators). Its header is a separate child
control, so the ListView is subclassed (`sys::control::HeaderSubclass`) to
intercept the header's `NM_CUSTOMDRAW` and paint it dark too.

## Adding a control

Every new control must follow the widget layer: it holds a `Control`, maps its
events to the app's `Msg`, implements `Themed`, and its PR includes a light and
a dark screenshot. #33 turns the steps below into that shape. Until it lands,
these are the platform-level mechanics every control still needs:

1. Add a `sys::control` helper for the raw message(s) you need; keep it safe
   and document each `unsafe` block.
2. Add `controls/<name>.rs`: a struct owning a child `HWND` (create via
   `controls::create_child`), an inner state implementing
   `registry::ControlEvents` if it needs owner-data/custom-draw, and a `Drop`
   that unregisters + destroys.
3. Decode application-level notifications into a `…Event` enum and add a
   `Notify::…` variant in `message.rs` + `sys::message::decode_notify`.
4. Re-export it from `lib.rs` (and `prelude`), and exercise it in
   `examples/demo/` + `tests/smoke.rs`.

## Dark theming notes

- Rows/header are painted by us, so their colours come from `ListViewTheme`.
- The scroll bar is themed with `SetWindowTheme(hwnd, "DarkMode_Explorer",
  null)` (documented API, works on Win10/11). The native header does **not**
  honour it, which is why the header is owner-drawn.
- The native status bar exposes no text colour, so `StatusBar` is owner-drawn
  too.
- DPI: `win32ui::init()` opts into per-monitor-v2 awareness; layout values are
  passed through `dpi_scale`. The example binaries embed a Common Controls v6 +
  DPI manifest (`win32ui.rc` / `win32ui.manifest`, via `build.rs`).

## Running

```
cargo run --example demo
```

Set `WIN32UI_DEMO_AUTOCLOSE_MS=4000` to have the demo quit itself (used for
headless smoke runs). The demo's data deliberately includes CJK, astral-plane
emoji, combining marks and RTL text; `tests/` and `src/controls/listview.rs`
unit tests assert those round-trip through the UTF-16 owner-data path.
