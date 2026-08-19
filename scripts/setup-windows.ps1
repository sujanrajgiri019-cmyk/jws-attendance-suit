# JWS Attendance — one-time development setup for Windows.
#
# Installs the two toolchains the project needs (Rust and Node), then builds it.
# Safe to run more than once: anything already present is skipped.
#
# Run in PowerShell from the project folder:
#     powershell -ExecutionPolicy Bypass -File scripts\setup-windows.ps1

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

function Write-Step($text) { Write-Host "`n=== $text" -ForegroundColor Cyan }
function Write-Ok($text)   { Write-Host "  OK  $text" -ForegroundColor Green }
function Write-Warn($text) { Write-Host "  !!  $text" -ForegroundColor Yellow }

function Test-Command($name) {
  return [bool](Get-Command $name -ErrorAction SilentlyContinue)
}

Write-Host @"

  JWS Attendance — build setup
  Janapremi World School

"@ -ForegroundColor White

# ---------------------------------------------------------------------------
Write-Step 'Checking for winget'
if (-not (Test-Command winget)) {
  Write-Warn 'winget was not found.'
  Write-Host @'
  winget ships with Windows 10 (1809+) and Windows 11 as "App Installer".
  Install it from the Microsoft Store, or install the tools by hand:

    Rust  : https://rustup.rs
    Node  : https://nodejs.org  (LTS)
    Build : Visual Studio 2022 Build Tools, "Desktop development with C++"

  Then run this script again.
'@
  exit 1
}
Write-Ok 'winget is available'

# ---------------------------------------------------------------------------
Write-Step 'Visual Studio C++ build tools'
# Rust on Windows links with the MSVC toolchain. Without this, `cargo build`
# fails with "link.exe not found", which is a confusing first error.
$vsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$haveMsvc = $false
if (Test-Path $vsWhere) {
  $found = & $vsWhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
  if ($found) { $haveMsvc = $true }
}
if ($haveMsvc) {
  Write-Ok 'MSVC build tools already installed'
} else {
  Write-Host '  Installing Visual Studio 2022 Build Tools (this is the big one, ~2 GB)...'
  winget install --id Microsoft.VisualStudio.2022.BuildTools --silent --accept-package-agreements --accept-source-agreements `
    --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
  Write-Ok 'MSVC build tools installed'
}

# ---------------------------------------------------------------------------
Write-Step 'WebView2 runtime'
# Tauri renders the interface in WebView2. Windows 11 has it; Windows 10 may not.
$wv = Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -ErrorAction SilentlyContinue
if ($wv) {
  Write-Ok "WebView2 runtime present (version $($wv.pv))"
} else {
  Write-Host '  Installing the WebView2 runtime...'
  winget install --id Microsoft.EdgeWebView2Runtime --silent --accept-package-agreements --accept-source-agreements
  Write-Ok 'WebView2 runtime installed'
}

# ---------------------------------------------------------------------------
Write-Step 'Rust'
if (Test-Command rustc) {
  Write-Ok "Rust already installed ($(rustc --version))"
} else {
  Write-Host '  Installing Rust...'
  winget install --id Rustlang.Rustup --silent --accept-package-agreements --accept-source-agreements
  $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
  Write-Ok 'Rust installed'
}

# ---------------------------------------------------------------------------
Write-Step 'Node.js'
if (Test-Command node) {
  Write-Ok "Node already installed ($(node --version))"
} else {
  Write-Host '  Installing Node.js LTS...'
  winget install --id OpenJS.NodeJS.LTS --silent --accept-package-agreements --accept-source-agreements
  $env:Path = "$env:ProgramFiles\nodejs;$env:Path"
  Write-Ok 'Node installed'
}

# ---------------------------------------------------------------------------
Write-Step 'Project dependencies'
Push-Location $root
try {
  if (-not (Test-Command npm)) {
    Write-Warn 'npm is not on PATH yet. Close this window, open a new PowerShell, and run the script again.'
    exit 1
  }
  npm install
  Write-Ok 'npm packages installed'

  Write-Step 'Running the tests'
  cargo test --workspace
  npm test
  Write-Ok 'All tests passed'

  Write-Step 'Building the interface'
  npm run build
  Write-Ok 'Interface built'
} finally {
  Pop-Location
}

Write-Host @"

  Setup complete.

  To run the app while working on it:
      npm start

  To produce the Windows installer:
      npm run package

  The installer lands in:
      src-tauri\target\release\bundle\nsis\

"@ -ForegroundColor Green
