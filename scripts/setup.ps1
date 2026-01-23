$ErrorActionPreference = "Stop"

$RootDir = Resolve-Path "$PSScriptRoot\.."
$SecretsFile = "$RootDir\config\.secrets.json"
$LogDir = "$RootDir\logs"
$BridgeExe = "$RootDir\bin\lunu-bridge.exe"

Write-Host "=== Lunu Toolchain Setup ===" -ForegroundColor Cyan

if (-not (Test-Path $LogDir)) {
    New-Item -ItemType Directory -Path $LogDir | Out-Null
}

if (-not (Test-Path $BridgeExe)) {
    Write-Error "Bridge executable not found at $BridgeExe. Rebuild the toolchain."
}

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
Write-Host "  $BridgeExe" -ForegroundColor White
