# Scripts to install Lunu to User Path
$ErrorActionPreference = "Stop"

$BinPath = Join-Path $PSScriptRoot "..\bin"
$AbsBinPath = (Resolve-Path $BinPath).Path

Write-Host "Installing Lunu to PATH..." -ForegroundColor Cyan
Write-Host "Target: $AbsBinPath" -ForegroundColor Gray

# Get current User PATH
$CurrentPath = [Environment]::GetEnvironmentVariable("Path", "User")

# Check if already exists
if ($CurrentPath -like "*$AbsBinPath*") {
    Write-Host "Success: Lunu is already in your PATH." -ForegroundColor Green
} else {
    # Append to PATH
    $NewPath = "$CurrentPath;$AbsBinPath"
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    
    # Update current session as well so it works immediately (in this process)
    $env:PATH += ";$AbsBinPath"
    
    Write-Host "Success: Added to PATH!" -ForegroundColor Green
    Write-Host "Note: You may need to restart your terminal (or VS Code) for changes to take effect globally." -ForegroundColor Yellow
}
