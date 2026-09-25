---
name: sandbox-tests
description: Run or screenshot win32ui's UI/integration tests (or a win32ui client app's tests or binary) inside Windows Sandbox so they never steal focus or input from the desktop the user or other agents are using. Use whenever you need to run `cargo test`, a single test binary or the demo on a Windows host, or to take a screenshot of an app to see how it looks.
---

# Running UI tests in Windows Sandbox

The integration tests create top-level windows, call `SetForegroundWindow` and
send synthetic input. On a shared desktop, that interferes with the user and with
other agents. Run them in Windows Sandbox with `scripts/sandbox/run.ps1`
(full docs: `docs/sandbox-testing.md`).

## Commands

```powershell
scripts\sandbox\run.ps1                                              # whole suite
scripts\sandbox\run.ps1 -CargoArgs '--test','slider'                 # one test binary
scripts\sandbox\run.ps1 -CargoArgs '--test','slider' -TestArgs 'drag'  # libtest filter
scripts\sandbox\run.ps1 -CargoArgs '--features','wgc'                # wgc capture tests
scripts\sandbox\run.ps1 -Build -CargoArgs '--example','demo' -Env @{ WIN32UI_DEMO_AUTOCLOSE_MS = '4000' }
```

## Screenshots

```powershell
scripts\sandbox\run.ps1 -Build -CargoArgs '--example','demo' -Screenshot            # out\demo.png
scripts\sandbox\run.ps1 -Build -CargoArgs '--example','demo' -Screenshot -ScreenshotDelayMs 6000
```

This starts the executable, waits, captures the whole sandbox desktop and stops
the executable. Open the PNG it prints (`Screenshot: …`) with your image-reading
tool to look at it. Use it instead of capturing the host desktop: it can't
raise windows or disturb anyone. If a window isn't painted yet, raise the delay.

For a client app, add `-ManifestPath <app>\Cargo.toml`, or pass `-Exe <path>` for
executables that are already built.

## Rules

- Exit code 0 means every executable passed. Logs are printed and kept in
  `target\sandbox\sandbox-stage\out\<binary>.log`, and `summary.txt` has one
  line per binary. Read the failing binary's log before you change any code.
- Screenshots or files a test writes must go under `C:\stage\out` inside the
  sandbox. Pass it through `-Env`, for example
  `-Env @{ WIN32UI_SLIDER_SHOTS = 'C:\stage\out' }`. They appear in the host's
  `out\` folder.
- Only one sandbox can run at a time. If the script says one is already
  running, it probably belongs to someone else: wait for it or report it. Never
  kill it.
- Don't use `-Keep` in unattended runs. It leaves the VM open.
- If Windows Sandbox isn't enabled, the script prints the command that enables
  it (admin rights and a reboot). Ask the user; don't enable it yourself.
- The non-UI checks (`cargo fmt`, `check`, `clippy`) don't need the sandbox.
  Run them on the host as usual.
