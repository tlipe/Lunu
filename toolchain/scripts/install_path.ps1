$BinPath = Resolve-Path "$PSScriptRoot\..\..\bin"
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")

if ($UserPath -notlike "*$BinPath*") {
    Write-Host "Adding Lunu to the user PATH..." -ForegroundColor Cyan
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$BinPath", "User")
    Write-Host "Success! Restart your terminal (VS Code/PowerShell) to use 'lunu' globally." -ForegroundColor Green
} else {
    Write-Host "Lunu is already on your PATH." -ForegroundColor Yellow
}
