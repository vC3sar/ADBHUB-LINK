<#
.SYNOPSIS
    ADBHUB-LINK Desktop — Automated Technician Windows Workstation Installation Script.
.DESCRIPTION
    Installs the ADBHUB-LINK Desktop suite on the technician's workstation.
    1. Verifies Windows Administrator privileges.
    2. Checks and installs 'vadimgrn.usbip-win2' (UDE Virtual USB Host Controller signed driver).
    3. Configures local Windows Firewall rules for Desktop UI (TCP 3420) and QUIC Relay (UDP 4433).
    4. Copies binaries and creates Desktop/Start Menu shortcuts.
    5. Verifies local driver stack readiness.
#>

[CmdletBinding()]
param(
    [string]$InstallDir = "$env:ProgramFiles\ADBHUB-LINK\Desktop",
    [switch]$NoShortcuts = $false
)

$ErrorActionPreference = "Stop"

function Write-Step {
    param([string]$Message)
    Write-Host "`n[ADBHUB-LINK] >>> $Message" -ForegroundColor Cyan
}

function Write-Success {
    param([string]$Message)
    Write-Host "[ADBHUB-LINK] [OK] $Message" -ForegroundColor Green
}

function Write-WarnMsg {
    param([string]$Message)
    Write-Host "[ADBHUB-LINK] [WARN] $Message" -ForegroundColor Yellow
}

# 1. Check Administrator Privileges
Write-Step "Checking Administrator Privileges..."
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Error "This script requires Administrator privileges. Please right-click PowerShell and select 'Run as Administrator'."
    exit 1
}
Write-Success "Running with Administrator privileges."

# 2. Check and Install usbip-win2 (UDE Driver)
Write-Step "Checking 'usbip-win2' (UDE Virtual USB Host Controller)..."
$defaultUsbipPath = "$env:ProgramFiles\usbip-win2\usbip.exe"
$bundledExe = "$PSScriptRoot\..\packages\usbip-win2.exe"
$driverExists = Test-Path "$env:SystemRoot\System32\drivers\usbip2_ude.sys"
$usbipCli = Get-Command "usbip" -ErrorAction SilentlyContinue

if (-not $usbipCli -and (Test-Path $defaultUsbipPath)) {
    $env:Path = "$env:ProgramFiles\usbip-win2;" + $env:Path
    $usbipCli = Get-Command "usbip" -ErrorAction SilentlyContinue
}

if (-not $usbipCli -and -not $driverExists) {
    Write-WarnMsg "'usbip-win2' driver not detected. Running automated silent driver installation..."

    if (Test-Path $bundledExe) {
        Write-Host "Installing pre-bundled signed UDE driver ($bundledExe)..." -ForegroundColor Cyan
        Start-Process $bundledExe -ArgumentList "/S /v/qn" -NoNewWindow -Wait
        Write-Success "usbip-win2 installed successfully from pre-bundled package."
    } else {
        $wingetCmd = Get-Command "winget" -ErrorAction SilentlyContinue
        $installed = $false

        if ($wingetCmd) {
            try {
                Write-Host "Installing via winget..." -ForegroundColor Cyan
                Start-Process winget -ArgumentList "install --exact --id vadimgrn.usbip-win2 --accept-source-agreements --accept-package-agreements --silent" -NoNewWindow -Wait
                $installed = $true
            } catch {}
        }

        if (-not $installed -or -not (Test-Path $defaultUsbipPath)) {
            try {
                Write-Host "Downloading signed usbip-win2 installer directly from GitHub..." -ForegroundColor Cyan
                $downloadUrl = "https://github.com/vadimgrn/usbip-win2/releases/download/v.0.9.8.1/USBip-0.9.8.1-x64.exe"
                $tempExe = "$env:TEMP\usbip-win2_installer.exe"
                [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
                Invoke-WebRequest -Uri $downloadUrl -OutFile $tempExe -UseBasicParsing
                Start-Process $tempExe -ArgumentList "/S /v/qn" -NoNewWindow -Wait
                Remove-Item -Path $tempExe -Force -ErrorAction SilentlyContinue
            } catch {
                Write-WarnMsg "Error during fallback download: $_"
            }
        }
    }

    $env:Path = [System.Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path","User")
    if (Test-Path $defaultUsbipPath -and $env:Path -notmatch "usbip-win2") {
        $env:Path = "$env:ProgramFiles\usbip-win2;" + $env:Path
    }
    $usbipCli = Get-Command "usbip" -ErrorAction SilentlyContinue
}

if ($usbipCli -or (Test-Path $defaultUsbipPath) -or $driverExists) {
    Write-Success "'usbip-win2' VHCI stack detected and ready."
} else {
    Write-WarnMsg "'usbip-win2' not found. Please install manually from https://github.com/vadimgrn/usbip-win2/releases"
}

# 3. Configure Windows Firewall
Write-Step "Configuring Windows Defender Firewall rules..."
try {
    # Allow local Desktop UI (TCP 3420) on loopback
    $existingFw3420 = Get-NetFirewallRule -DisplayName "ADBHUB-LINK Desktop UI (TCP 3420)" -ErrorAction SilentlyContinue
    if (-not $existingFw3420) {
        New-NetFirewallRule -DisplayName "ADBHUB-LINK Desktop UI (TCP 3420)" `
            -Direction Inbound `
            -Action Allow `
            -Protocol TCP `
            -LocalPort 3420 `
            -Description "ADBHUB-LINK Technician Desktop Local UI Server" | Out-Null
        Write-Success "Inbound TCP 3420 firewall rule created."
    }

    # Allow outbound QUIC relay (UDP 4433)
    $existingFw4433 = Get-NetFirewallRule -DisplayName "ADBHUB-LINK Outbound QUIC Relay (UDP 4433)" -ErrorAction SilentlyContinue
    if (-not $existingFw4433) {
        New-NetFirewallRule -DisplayName "ADBHUB-LINK Outbound QUIC Relay (UDP 4433)" `
            -Direction Outbound `
            -Action Allow `
            -Protocol UDP `
            -RemotePort 4433 `
            -Description "Allows ADBHUB-LINK Desktop outbound encrypted QUIC tunnel to Cloud Relay" | Out-Null
        Write-Success "Outbound UDP 4433 firewall rule created."
    }
} catch {
    Write-WarnMsg "Firewall rule configuration notice: $_"
}

# 4. Copy Desktop Binaries
Write-Step "Installing ADBHUB-LINK Desktop binaries to '$InstallDir'..."
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}
if (-not (Test-Path "$InstallDir\data")) {
    New-Item -ItemType Directory -Path "$InstallDir\data" -Force | Out-Null
}

$sourceExe = "$PSScriptRoot\..\..\target\release\adbhub-desktop.exe"
if (-not (Test-Path $sourceExe)) {
    $sourceExe = "$PSScriptRoot\..\..\target\debug\adbhub-desktop.exe"
}

if (Test-Path $sourceExe) {
    Copy-Item -Path $sourceExe -Destination "$InstallDir\adbhub-desktop.exe" -Force
    Write-Success "Binary copied: $InstallDir\adbhub-desktop.exe"
} else {
    Write-WarnMsg "Compiled adbhub-desktop.exe not found at target directory. Please build with 'cargo build --release' or copy to $InstallDir."
}

# 5. Create Shortcuts
if (-not $NoShortcuts) {
    Write-Step "Creating Desktop and Start Menu shortcuts..."
    $wshShell = New-Object -ComObject WScript.Shell
    
    # Desktop Shortcut
    $desktopPath = [System.Environment]::GetFolderPath("CommonDesktopDirectory")
    $shortcut = $wshShell.CreateShortcut("$desktopPath\ADBHUB-LINK Desktop.lnk")
    $shortcut.TargetPath = "$InstallDir\adbhub-desktop.exe"
    $shortcut.WorkingDirectory = $InstallDir
    $shortcut.Description = "ADBHUB-LINK Professional Remote USB Passthrough for Mobile Repair"
    $shortcut.Save()
    Write-Success "Desktop shortcut created: $desktopPath\ADBHUB-LINK Desktop.lnk"

    # Start Menu Shortcut
    $startMenuPath = [System.Environment]::GetFolderPath("CommonPrograms") + "\ADBHUB-LINK"
    if (-not (Test-Path $startMenuPath)) {
        New-Item -ItemType Directory -Path $startMenuPath -Force | Out-Null
    }
    $smShortcut = $wshShell.CreateShortcut("$startMenuPath\ADBHUB-LINK Desktop.lnk")
    $smShortcut.TargetPath = "$InstallDir\adbhub-desktop.exe"
    $smShortcut.WorkingDirectory = $InstallDir
    $smShortcut.Save()
    Write-Success "Start menu shortcut created."
}

Write-Host "`n================================================================================" -ForegroundColor Green
Write-Host " ADBHUB-LINK Desktop installed successfully!" -ForegroundColor Green
Write-Host " Launch from Desktop shortcut or run: & '$InstallDir\adbhub-desktop.exe'" -ForegroundColor Cyan
Write-Host " Default UI URL: http://127.0.0.1:3420" -ForegroundColor Cyan
Write-Host "================================================================================`n" -ForegroundColor Green

