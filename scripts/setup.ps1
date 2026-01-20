# Lunu Setup Script
$ErrorActionPreference = "Stop"

$RootDir = Resolve-Path "$PSScriptRoot\.."
$VenvDir = "$RootDir\.venv"
$ReqFile = "$RootDir\requirements.txt"
$SecretsFile = "$RootDir\config\.secrets.json"

Write-Host "=== Lunu Toolchain Setup ===" -ForegroundColor Cyan

# 1. Check Python
try {
    $pyVersion = python --version
    Write-Host "Found Python: $pyVersion" -ForegroundColor Green
} catch {
    Write-Error "Python not found in PATH. Please install Python 3.10+."
}

# 2. Create Virtual Environment
if (-not (Test-Path $VenvDir)) {
    Write-Host "Creating Unified Virtual Environment..." -ForegroundColor Yellow
    python -m venv $VenvDir
} else {
    Write-Host "Virtual Environment already exists." -ForegroundColor Gray
}

# 3. Install Dependencies
Write-Host "Installing/Updating Dependencies..." -ForegroundColor Yellow
$Pip = "$VenvDir\Scripts\pip.exe"
& $Pip install --upgrade pip | Out-Null
& $Pip install -r $ReqFile

# 4. Generate Secrets
if (-not (Test-Path $SecretsFile)) {
    Write-Host "Generating Secure API Key..." -ForegroundColor Yellow
    $Key = -join ((65..90) + (97..122) + (48..57) | Get-Random -Count 32 | % {[char]$_})
    $JsonContent = @{ api_key = $Key } | ConvertTo-Json
    $JsonContent | Out-File -FilePath $SecretsFile -Encoding utf8
    
    Write-Host " "
    Write-Host "!!! NEW SECURITY KEY GENERATED !!!" -ForegroundColor Red
    Write-Host "Key: $Key" -ForegroundColor White
    Write-Host "Saved to: $SecretsFile" -ForegroundColor Gray
    Write-Host "Use this key in your Luau clients (X-LUNU-KEY header)." -ForegroundColor Gray
    Write-Host " "
}

Write-Host "=== Setup Complete ===" -ForegroundColor Green
Write-Host "To start the Lunu Bridge Server:"
Write-Host "  $VenvDir\Scripts\python.exe src/bridge/server.py" -ForegroundColor White
