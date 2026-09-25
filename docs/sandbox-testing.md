# Running UI tests without taking over your desktop

The integration tests create real top-level windows, move focus
(`SetForegroundWindow`) and send synthetic input. On your own desktop that
steals focus while you type, and your typing can make tests flaky.
`scripts/sandbox/run.ps1` runs them inside **Windows Sandbox**, a throwaway
Hyper-V VM that ships with Windows. It has its own desktop, input queue and
foreground window.

## One-time setup (Windows 10/11 Pro, Enterprise or Education)

```powershell
# as administrator, then reboot
Enable-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM
```

You install nothing else. The sandbox doesn't need Rust or Visual Studio.

## Usage

```powershell
scripts\sandbox\run.ps1                                        # every test
scripts\sandbox\run.ps1 -CargoArgs '--test','slider' -TestArgs 'drag'
scripts\sandbox\run.ps1 -CargoArgs '--features','wgc'
scripts\sandbox\run.ps1 -Env @{ WIN32UI_SLIDER_SHOTS = 'C:\stage\out' }  # screenshots land in target\sandbox\sandbox-stage\out
```

How it works:

1. Builds on the host with `cargo test --no-run`, into `target\sandbox` with a
   static CRT (the sandbox image has no VC++ runtime).
2. Copies the test executables into `target\sandbox\sandbox-stage\bin`. This
   folder is mapped into the sandbox as `C:\stage`.
3. Starts a network-less sandbox with a vGPU (for D2D, OpenGL and DWM). Its logon
   command runs every executable and writes `out\<name>.log` and
   `out\summary.txt`. The host script then closes the sandbox itself, so you
   don't get the "Windows Sandbox has closed" dialog.
4. Prints the logs. It exits with code 1 if any executable failed.

A cold start takes about 15–30 s on top of the build. The sandbox window opens
once. You can leave it behind your other windows, because nothing inside it
touches the host's focus or input. If you minimize it, Windows may stop
rendering the VM's desktop, which breaks capture tests. Use `-Keep` to leave the
sandbox open afterwards so you can look around.

Limitations: only one sandbox can run at a time, and it needs hardware
virtualization. Windows Home doesn't include it (see the alternatives below).

## Client apps of win32ui

The script works with any Cargo project. Call it from your app's repository:

```powershell
# your app's tests
path\to\win32ui\scripts\sandbox\run.ps1 -ManifestPath .\Cargo.toml

# build and launch the app itself as a smoke test (give it an auto-close switch
# like the demo's WIN32UI_DEMO_AUTOCLOSE_MS, or it will run until the timeout)
path\to\win32ui\scripts\sandbox\run.ps1 -Build -CargoArgs '--bin','myapp' -Env @{ MYAPP_AUTOCLOSE_MS = '4000' }

# executables you already built
path\to\win32ui\scripts\sandbox\run.ps1 -Exe .\dist\myapp_tests.exe
```

You can also copy `run.ps1` into your repository. It has no dependency on
win32ui.

## CI

The `windows-latest` GitHub runner is already a disposable VM with an
interactive desktop, so the existing `ci.yml` jobs are sandboxed by design.
Push a branch to run the whole suite without touching your machine. Windows
Sandbox itself can't run on hosted runners (they don't allow nested
virtualization), so `run.ps1` is for local use.

## Alternatives

- **Hyper-V / VirtualBox VM with a Windows dev image**: works on Windows Home
  (VirtualBox) and allows several parallel VMs, but setup is slower.
- **A second RDP session to your own machine** (a local user with an
  autologon session): tests get their own desktop, but you need a Windows
  Server or multi-session SKU, or a second account plus RDP wrapper hacks.
  Windows Sandbox is simpler.
