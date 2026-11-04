# One-line installer for flipfile on Windows 10/11 (PowerShell)
# Usage: irm https://raw.githubusercontent.com/wcksnrdn/flipfile/main/scripts/install.ps1 | iex

$ErrorActionPreference = "Stop"
$Repo = "wcksnrdn/flipfile"

Write-Host "=> Fetching latest flipfile release from GitHub ($Repo)..." -ForegroundColor Cyan
$Release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
$Tag = $Release.tag_name
$Version = $Tag.TrimStart("v")

$Url = "https://github.com/$Repo/releases/download/$Tag/flipfile_${Version}_x64-setup.exe"
$OutFile = Join-Path $env:TEMP "flipfile_${Version}_x64-setup.exe"

Write-Host "=> Downloading flipfile $Tag installer..." -ForegroundColor Cyan
Invoke-WebRequest -Uri $Url -OutFile $OutFile

Write-Host "=> Launching flipfile installer..." -ForegroundColor Green
Start-Process -FilePath $OutFile -Wait
Remove-Item -Path $OutFile -ErrorAction SilentlyContinue
Write-Host "✓ flipfile $Tag installation finished!" -ForegroundColor Green
