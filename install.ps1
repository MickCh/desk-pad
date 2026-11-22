# Build desk-pad and install it for the current user:
# %LOCALAPPDATA%\Programs\desk-pad\desk-pad.exe
# Quit a running desk-pad first (tray icon → Quit): Windows locks a running exe.
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$dest = Join-Path $env:LOCALAPPDATA "Programs\desk-pad"
$exe = Join-Path $dest "desk-pad.exe"
if (Get-Process desk-pad -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe }) {
    throw "desk-pad is running from ${exe}: quit it first (tray icon → Quit)."
}

# SDL2 is built from source: CMake must be on PATH. Use the one that comes
# with Visual Studio if there is no other.
if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        $vs = & $vswhere -latest -property installationPath
        $cmake = Join-Path $vs "Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin"
        if (Test-Path $cmake) { $env:PATH = "$cmake;$env:PATH" }
    }
}

cargo build --release
if ($LASTEXITCODE -ne 0) { throw "build failed" }
New-Item -ItemType Directory -Force -Path $dest | Out-Null
Copy-Item "target\release\desk-pad.exe" $exe -Force
Write-Host "Installed: $exe"
Write-Host "Start it (an icon appears in the notification area), then tick"
Write-Host "'Start with Windows' in its menu to have it start at logon."
