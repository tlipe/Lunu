$ErrorActionPreference = "Stop"

Write-Host "Building Lunu CLI..." -ForegroundColor Cyan

# Check for Rust
if (-not (Get-Command "cargo" -ErrorAction SilentlyContinue)) {
    Write-Error "Rust (cargo) not found. Please install Rust from https://rustup.rs/"
    exit 1
}

$LutePayload = "$PSScriptRoot\..\resources\lute.exe"
if (-not (Test-Path $LutePayload)) {
    Write-Error "Embedded Lute runtime not found at $LutePayload. Place lute.exe there before building."
    exit 1
}

$BinDir = "$PSScriptRoot\..\..\bin"
if (-not (Test-Path $BinDir)) {
    New-Item -ItemType Directory -Path $BinDir | Out-Null
}

cd "$PSScriptRoot\.."

# 1. Build Builder & Stub (in ../builder)
Write-Host "Compiling builder binaries..."
Push-Location "$PSScriptRoot\..\..\builder"
cargo build --release
if ($LASTEXITCODE -ne 0) { Write-Error "Builder build failed"; exit 1 }
Pop-Location

# 2. Build Installer (which embeds all 3 binaries)
Write-Host "Compiling installer (embedding payloads)..."
cargo build --release --bin lunu
if ($LASTEXITCODE -ne 0) { Write-Error "Installer build failed"; exit 1 }

# 3. Copy only the installer to bin
$InstallerTarget = "$PSScriptRoot\..\target\release\lunu.exe"

if (Test-Path $InstallerTarget) {
    Copy-Item $InstallerTarget "$BinDir\lunu.exe" -Force
    Write-Host "Build Successful!" -ForegroundColor Green
    Write-Host "Installer created at: $BinDir\lunu.exe" -ForegroundColor White
    Write-Host "You only need to distribute 'lunu.exe'." -ForegroundColor Yellow
} else {
    Write-Error "Installer binary not found at $InstallerTarget"
}
