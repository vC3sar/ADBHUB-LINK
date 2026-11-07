<#
.SYNOPSIS
    ADBHUB-LINK Driver Bundler Script.
.DESCRIPTION
    Downloads the official signed driver packages (usbipd-win MSI and usbip-win2 installer)
    into installer/packages/ so they are bundled directly into the offline installer package.
#>

[CmdletBinding()]
param()

$packagesDir = "$PSScriptRoot\packages"
if (-not (Test-Path $packagesDir)) {
    New-Item -ItemType Directory -Path $packagesDir -Force | Out-Null
}

[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$usbipdUrl = "https://github.com/dorssel/usbipd-win/releases/download/v5.3.0/usbipd-win_5.3.0_x64.msi"
$usbipdDest = "$packagesDir\usbipd-win.msi"

$usbipWin2Url = "https://github.com/vadimgrn/usbip-win2/releases/download/v.0.9.8.1/USBip-0.9.8.1-x64.exe"
$usbipWin2Dest = "$packagesDir\usbip-win2.exe"

Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host " Pre-bundling Signed Drivers for 100% Offline Automated Installation" -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan

if (-not (Test-Path $usbipdDest)) {
    Write-Host "Downloading usbipd-win (Host Export WHQL driver)..." -ForegroundColor Yellow
    Invoke-WebRequest -Uri $usbipdUrl -OutFile $usbipdDest -UseBasicParsing
    Write-Host "  -> Bundled: $usbipdDest" -ForegroundColor Green
} else {
    Write-Host "  -> Already bundled: $usbipdDest" -ForegroundColor Green
}

if (-not (Test-Path $usbipWin2Dest)) {
    Write-Host "Downloading usbip-win2 (Technician UDE driver)..." -ForegroundColor Yellow
    Invoke-WebRequest -Uri $usbipWin2Url -OutFile $usbipWin2Dest -UseBasicParsing
    Write-Host "  -> Bundled: $usbipWin2Dest" -ForegroundColor Green
} else {
    Write-Host "  -> Already bundled: $usbipWin2Dest" -ForegroundColor Green
}

Write-Host "`nAll signed drivers bundled successfully in installer/packages/" -ForegroundColor Green

