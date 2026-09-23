# Agent conventions

Read this and [README.md](README.md) before writing any code. The README's
"Invariants" and "Adding a control" sections are part of these rules.

## Invariants (non-negotiable)

- **`unsafe` only in `src/sys/`.** Every other module starts with
  `#![forbid(unsafe_code)]`. Each `unsafe` block carries a `// SAFETY:` comment
  explaining why its contract holds.
- **No `windows` types in the public API.** They may be private fields; callers
  only ever see `win32ui` types.
- **Controls own their child `HWND`** and destroy it (and unregister from
  `controls::registry`) in `Drop`.
- **Small files.** Aim under 300 lines, hard limit 400. If a change would push a
  file past that, split it along a real seam first. New raw-Win32 helpers for a
  new control go in their own `src/sys/<name>.rs` rather than growing
  `sys/control.rs`.
- **Win32 constants come from the `windows` crate or the SDK header**, never from
  memory. If you must mirror one as a literal (as `gdi/paint.rs` does for `DT_*`),
  say which header it comes from.

## Quality bar

- Idiomatic Rust: builders over long argument lists, enums over raw codes,
  RAII over manual cleanup. Match the existing module's naming and style.
- No allocation or GDI-object creation per item in paint/notification hot paths
  (`NM_CUSTOMDRAW`, `LVN_GETDISPINFO`, `WM_PAINT`) unless unavoidable; say so if it is.
- Public items get doc comments. Comments elsewhere only explain the non-obvious *why*.
- No dead code, no commented-out code. Nothing emusic-specific in new public API:
  this is a general-purpose library.
- Every new control or message is exercised in `examples/demo.rs` and has a
  test in `tests/` or a unit test.

## Checks (all must pass before opening a PR)

```bash
cargo fmt --all --check
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test
```

## Operational rules

- Running the demo: always set `WIN32UI_DEMO_AUTOCLOSE_MS=4000` so it exits on
  its own, and never leave a demo process running when you finish.
- A dependency's source lives under `~/.cargo/registry/src/*/<crate>-<version>/`.
  Never scan the filesystem (`find /`, `Get-ChildItem -Recurse C:\`) for it.
- Touch only the files your issue names; if you need to change a file another
  open issue owns, say so in the PR description instead of working around it.
- Do not merge your own PR.
