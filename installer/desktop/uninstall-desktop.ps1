<#
.SYNOPSIS
    ADBHUB-LINK Desktop — Automated Technician Workstation Uninstallation Script.
.DESCRIPTION
    Detaches any active virtual VHCI ports, terminates Desktop processes, removes shortcuts,
    and cleans up desktop files.
#>

[CmdletBinding()]
param(
    [string]$InstallDir = "$env:ProgramFiles\ADBHUB-LINK\Desktop"
)

$ErrorActionPreference = "Continue"

Write-Host "`n[ADBHUB-LINK] >>> Starting ADBHUB-LINK Desktop Uninstallation..." -ForegroundColor Cyan

# 1. Check Administrator Privileges
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Error "This script requires Administrator privileges. Please right-click PowerShell and select 'Run as Administrator'."
    exit 1
}

# 2. Terminate running adbhub-desktop processes
Write-Host "[ADBHUB-LINK] Terminating running adbhub-desktop processes..." -ForegroundColor Cyan
Get-Process "adbhub-desktop" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

# 3. Detach all virtual VHCI ports if usbip CLI is available
$usbipCli = Get-Command "usbip" -ErrorAction SilentlyContinue
if ($usbipCli) {
    Write-Host "[ADBHUB-LINK] Detaching active VHCI ports..." -ForegroundColor Cyan
    try {
        & usbip port | ForEach-Object {
            if ($_ -match "Port (\d+):") {
                $p = $matches[1]
                Write-Host " Detaching Port $p..."
                & usbip detach -p $p 2>$null
            }
        }
    } catch {}
}

# 4. Remove shortcuts
Write-Host "[ADBHUB-LINK] Removing shortcuts..." -ForegroundColor Cyan
$desktopLnk = [System.Environment]::GetFolderPath("CommonDesktopDirectory") + "\ADBHUB-LINK Desktop.lnk"
if (Test-Path $desktopLnk) { Remove-Item -Path $desktopLnk -Force -ErrorAction SilentlyContinue }

$smDir = [System.Environment]::GetFolderPath("CommonPrograms") + "\ADBHUB-LINK"
if (Test-Path $smDir) { Remove-Item -Path $smDir -Recurse -Force -ErrorAction SilentlyContinue }

# 5. Remove firewall rules
Write-Host "[ADBHUB-LINK] Removing firewall rules..." -ForegroundColor Cyan
Remove-NetFirewallRule -DisplayName "ADBHUB-LINK Desktop UI (TCP 3420)" -ErrorAction SilentlyContinue
Remove-NetFirewallRule -DisplayName "ADBHUB-LINK Outbound QUIC Relay (UDP 4433)" -ErrorAction SilentlyContinue

# 6. Clean installation directory (keeps ./data if desired or prompts)
if (Test-Path $InstallDir) {
    Write-Host "[ADBHUB-LINK] Removing installation directory: $InstallDir" -ForegroundColor Cyan
    Remove-Item -Path $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "`n[ADBHUB-LINK] [OK] ADBHUB-LINK Desktop uninstalled successfully.`n" -ForegroundColor Green

