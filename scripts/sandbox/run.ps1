<#
.SYNOPSIS
    Runs win32ui test binaries (or any Windows GUI executable) inside Windows
    Sandbox, so windows, focus changes and synthetic input stay off your desktop.

.DESCRIPTION
    1. Builds on the host (`cargo test --no-run`, or `cargo build`) into a
       separate target dir with a static CRT, because the sandbox image has no
       Visual C++ runtime.
    2. Stages the executables and a runner script in `<target>\sandbox-stage`.
    3. Launches a disposable, network-less Windows Sandbox that maps the stage
       folder, runs every executable, writes logs to `out\` and shuts down.
    4. Prints the logs and exits non-zero if anything failed.

    Nothing is installed in the sandbox and nothing survives it.

.EXAMPLE
    # All of win32ui's tests
    scripts\sandbox\run.ps1

.EXAMPLE
    # One integration test binary, with the wgc feature, filtered
    scripts\sandbox\run.ps1 -CargoArgs '--test','slider','--features','wgc' -TestArgs 'drag'

.EXAMPLE
    # A client app's tests (run from its workspace, pointing at this script)
    ..\win32ui\scripts\sandbox\run.ps1 -ManifestPath .\Cargo.toml

.EXAMPLE
    # A client app's binary: run it for a smoke test instead of `cargo test`
    ..\win32ui\scripts\sandbox\run.ps1 -Build -CargoArgs '--bin','myapp' -Env @{ MYAPP_AUTOCLOSE_MS = '4000' }
#>
[CmdletBinding()]
param(
    # Cargo.toml of the crate or workspace to build.
    [string]$ManifestPath = (Join-Path $PSScriptRoot '..\..\Cargo.toml'),
    # Extra cargo arguments, e.g. '--test','slider' or '--features','wgc'.
    [string[]]$CargoArgs = @(),
    # Arguments passed to every executable (libtest filters, --ignored, ...).
    [string[]]$TestArgs = @(),
    # `cargo build` and run the produced binaries instead of `cargo test`.
    [switch]$Build,
    # Pre-built executables to run; skips cargo entirely.
    [string[]]$Exe = @(),
    # Environment variables set inside the sandbox before the runs.
    [hashtable]$Env = @{},
    # Minutes to wait for the sandbox before giving up.
    [int]$TimeoutMinutes = 20,
    [int]$MemoryMB = 4096,
    # Leave the sandbox open after the runs, to inspect it.
    [switch]$Keep
)
$ErrorActionPreference = 'Stop'

$sandboxExe = Join-Path $env:windir 'System32\WindowsSandbox.exe'
if (-not (Test-Path $sandboxExe)) {
    throw "Windows Sandbox is not enabled. Run once as admin, then reboot:`n" +
          "  Enable-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM"
}
if (Get-Process -Name WindowsSandboxRemoteSession, WindowsSandboxClient -ErrorAction SilentlyContinue) {
    throw 'A Windows Sandbox is already running; only one can run at a time. Close it first.'
}

$manifest = (Resolve-Path $ManifestPath).Path
$targetDir = Join-Path (Split-Path $manifest) 'target\sandbox'
$stage = Join-Path $targetDir 'sandbox-stage'
$bin = Join-Path $stage 'bin'
$out = Join-Path $stage 'out'
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $bin, $out | Out-Null

# --- Build on the host ------------------------------------------------------
if ($Exe.Count -eq 0) {
    $env:CARGO_TARGET_DIR = $targetDir
    # The sandbox has no vcruntime140.dll; a static CRT makes the binaries self-contained.
    $env:RUSTFLAGS = "$env:RUSTFLAGS -C target-feature=+crt-static".Trim()
    $cmd = if ($Build) { @('build') } else { @('test', '--no-run') }
    $cargoOut = & cargo @cmd --manifest-path $manifest --message-format=json-render-diagnostics @CargoArgs
    if ($LASTEXITCODE -ne 0) { throw "cargo $($cmd -join ' ') failed" }
    $Exe = $cargoOut | ForEach-Object {
        if ($_ -notmatch '^\{') { return }
        $msg = $_ | ConvertFrom-Json
        if ($msg.reason -eq 'compiler-artifact' -and $msg.executable -and
            ($Build -or $msg.profile.test)) { $msg.executable }
    } | Sort-Object -Unique
}
if ($Exe.Count -eq 0) { throw 'Nothing to run.' }
foreach ($e in $Exe) { Copy-Item $e $bin }

# --- Runner executed inside the sandbox ------------------------------------
$envLines = ($Env.GetEnumerator() | ForEach-Object {
    "`$env:$($_.Key) = '$($_.Value -replace "'", "''")'"
}) -join "`n"
$argList = ($TestArgs | ForEach-Object { "'$($_ -replace "'", "''")'" }) -join ','
$shutdown = if ($Keep) { '' } else { 'shutdown.exe /s /t 0' }
@"
`$ErrorActionPreference = 'Continue'
`$env:RUST_BACKTRACE = '1'
$envLines
`$failed = 0
Get-ChildItem C:\stage\bin\*.exe | ForEach-Object {
    `$log = "C:\stage\out\`$(`$_.BaseName).log"
    & `$_.FullName @($argList) *> `$log
    "`$(`$_.Name) exit=`$LASTEXITCODE" | Add-Content C:\stage\out\summary.txt
    if (`$LASTEXITCODE -ne 0) { `$failed++ }
}
Set-Content C:\stage\out\done.txt `$failed
$shutdown
"@ | Set-Content (Join-Path $stage 'runner.ps1') -Encoding UTF8

# --- Sandbox configuration --------------------------------------------------
$wsb = Join-Path $targetDir 'tests.wsb'
@"
<Configuration>
  <Networking>Disable</Networking>
  <vGPU>Enable</vGPU>
  <ClipboardRedirection>Disable</ClipboardRedirection>
  <AudioInput>Disable</AudioInput>
  <VideoInput>Disable</VideoInput>
  <MemoryInMB>$MemoryMB</MemoryInMB>
  <MappedFolders>
    <MappedFolder>
      <HostFolder>$stage</HostFolder>
      <SandboxFolder>C:\stage</SandboxFolder>
      <ReadOnly>false</ReadOnly>
    </MappedFolder>
  </MappedFolders>
  <LogonCommand>
    <Command>powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Minimized -File C:\stage\runner.ps1</Command>
  </LogonCommand>
</Configuration>
"@ | Set-Content $wsb -Encoding UTF8

Write-Host "Running $($Exe.Count) executable(s) in Windows Sandbox..."
Start-Process $sandboxExe -ArgumentList "`"$wsb`""

$done = Join-Path $out 'done.txt'
$deadline = (Get-Date).AddMinutes($TimeoutMinutes)
while (-not (Test-Path $done) -and (Get-Date) -lt $deadline) { Start-Sleep -Seconds 2 }

Get-ChildItem $out -Filter *.log | ForEach-Object {
    Write-Host "===== $($_.BaseName) =====" -ForegroundColor Cyan
    Get-Content $_.FullName
}
if (-not (Test-Path $done)) {
    Get-Process -Name WindowsSandboxRemoteSession, WindowsSandboxClient -ErrorAction SilentlyContinue | Stop-Process -Force
    throw "Timed out after $TimeoutMinutes minutes; logs are in $out"
}
Get-Content (Join-Path $out 'summary.txt')
$failed = [int](Get-Content $done)
Write-Host "Logs: $out"
exit $(if ($failed -gt 0) { 1 } else { 0 })
