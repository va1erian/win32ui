# win32ui

A small, idiomatic Rust wrapper over the slice of Win32 that a native
desktop app needs: custom windows, the message loop, GDI painting, and a few
common controls. Originally extracted from the
[emusic](https://github.com/va1erian/emusic) frontend.

## Layout

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
`WM_NCCREATE`; the shared `window_proc` (in `sys::window`) decodes each raw
message into a typed `Message` and calls the handler. The box is reclaimed
once, on `WM_NCDESTROY`.

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

1. Add a `sys::control` helper for the raw message(s) you need; keep it safe
   and document each `unsafe` block.
2. Add `controls/<name>.rs`: a struct owning a child `HWND` (create via
   `controls::create_child`), an inner state implementing
   `registry::ControlEvents` if it needs owner-data/custom-draw, and a `Drop`
   that unregisters + destroys.
3. Decode application-level notifications into a `…Event` enum and add a
   `Notify::…` variant in `message.rs` + `sys::message::decode_notify`.
4. Re-export it from `lib.rs` (and `prelude`), and exercise it in
   `examples/demo.rs` + `tests/smoke.rs`.

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
