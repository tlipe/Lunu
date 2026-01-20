$BinPath = Resolve-Path "$PSScriptRoot\..\..\bin"
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")

if ($UserPath -notlike "*$BinPath*") {
    Write-Host "Adicionando Lunu ao PATH do usuário..." -ForegroundColor Cyan
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$BinPath", "User")
    Write-Host "Sucesso! Reinicie seu terminal (VS Code/PowerShell) para usar o comando 'lunu' globalmente." -ForegroundColor Green
} else {
    Write-Host "Lunu já está no seu PATH." -ForegroundColor Yellow
}
