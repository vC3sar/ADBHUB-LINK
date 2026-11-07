<#
.SYNOPSIS
    ADBHUB-LINK Agent — Automated Windows Host Uninstallation Script.
.DESCRIPTION
    Safely unbinds all USB devices, stops background services/tasks, cleans firewall rules,
    and removes agent files from the customer PC.
#>

[CmdletBinding()]
param(
    [string]$InstallDir = "$env:ProgramFiles\ADBHUB-LINK\Agent"
)

$ErrorActionPreference = "Continue"

Write-Host "`n[ADBHUB-LINK] >>> Starting ADBHUB-LINK Agent Uninstallation..." -ForegroundColor Cyan

# 1. Check Administrator Privileges
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Error "This script requires Administrator privileges. Please right-click PowerShell and select 'Run as Administrator'."
    exit 1
}

# 2. Stop and remove scheduled task
Write-Host "[ADBHUB-LINK] Stopping and removing background task..." -ForegroundColor Cyan
Stop-ScheduledTask -TaskName "ADBHUB-LINK-Agent" -ErrorAction SilentlyContinue
Unregister-ScheduledTask -TaskName "ADBHUB-LINK-Agent" -Confirm:$false -ErrorAction SilentlyContinue

# 3. Stop running agent processes
Write-Host "[ADBHUB-LINK] Terminating running adbhub-agent processes..." -ForegroundColor Cyan
Get-Process "adbhub-agent" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

# 4. Remove Windows Defender Firewall rules
Write-Host "[ADBHUB-LINK] Removing firewall rules..." -ForegroundColor Cyan
Remove-NetFirewallRule -DisplayName "ADBHUB-LINK Host USB/IP Export (TCP 3240)" -ErrorAction SilentlyContinue
Remove-NetFirewallRule -DisplayName "ADBHUB-LINK Outbound QUIC Relay (UDP 4433)" -ErrorAction SilentlyContinue

# 5. Clean installation files
if (Test-Path $InstallDir) {
    Write-Host "[ADBHUB-LINK] Removing installation directory: $InstallDir" -ForegroundColor Cyan
    Remove-Item -Path $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "`n[ADBHUB-LINK] [OK] ADBHUB-LINK Agent uninstalled successfully.`n" -ForegroundColor Green

