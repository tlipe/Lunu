$ErrorActionPreference = "Stop"

Write-Host "Building Lunu CLI..." -ForegroundColor Cyan

# Check for Rust
if (-not (Get-Command "cargo" -ErrorAction SilentlyContinue)) {
    Write-Error "Rust (cargo) not found. Please install Rust from https://rustup.rs/"
}

# Build
cd "$PSScriptRoot\.."
cargo build --release

if ($LASTEXITCODE -eq 0) {
    $BinDir = "$PSScriptRoot\..\..\bin"
    $Target = "$PSScriptRoot\..\target\release\lunu-cli.exe"
    
    if (-not (Test-Path $BinDir)) {
        New-Item -ItemType Directory -Path $BinDir | Out-Null
    }
    
    Copy-Item $Target "$BinDir\lunu.exe" -Force
    Write-Host "Build Successful!" -ForegroundColor Green
    Write-Host "Executable installed to: $BinDir\lunu.exe" -ForegroundColor White
    Write-Host "Add '$BinDir' to your PATH to use 'lunu' globally."
} else {
    Write-Error "Build failed."
}
