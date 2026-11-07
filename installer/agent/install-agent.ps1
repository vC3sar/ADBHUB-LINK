<#
.SYNOPSIS
    ADBHUB-LINK Agent — Automated Windows Host Installation Script.
.DESCRIPTION
    Installs the ADBHUB-LINK Agent on the customer/host Windows machine.
    1. Verifies Windows Administrator privileges.
    2. Checks and installs 'dorssel.usbipd-win' (signed WHQL/Attestation driver).
    3. Configures Windows Defender Firewall rules (TCP 3240 Host export, UDP 4433 QUIC Relay).
    4. Registers 'ADBHUB-LINK Agent' as a high-priority Windows startup service/task.
    5. Runs doctor diagnostic to verify end-to-end readiness.
#>

[CmdletBinding()]
param(
    [string]$InstallDir = "$env:ProgramFiles\ADBHUB-LINK\Agent",
    [string]$RelayHost = "127.0.0.1:4433",
    [switch]$NoService = $false
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

# 2. Check and Install usbipd-win prerequisite
Write-Step "Checking 'usbipd-win' prerequisite..."
$defaultUsbipdPath = "$env:ProgramFiles\usbipd-win\usbipd.exe"
$bundledMsi = "$PSScriptRoot\..\packages\usbipd-win.msi"
$usbipdCmd = Get-Command "usbipd" -ErrorAction SilentlyContinue

if (-not $usbipdCmd -and (Test-Path $defaultUsbipdPath)) {
    $env:Path = "$env:ProgramFiles\usbipd-win;" + $env:Path
    $usbipdCmd = Get-Command "usbipd" -ErrorAction SilentlyContinue
}

if (-not $usbipdCmd) {
    Write-WarnMsg "'usbipd-win' is not installed or not in PATH. Running automated silent driver installation..."

    if (Test-Path $bundledMsi) {
        Write-Host "Installing pre-bundled signed WHQL driver ($bundledMsi)..." -ForegroundColor Cyan
        $msiProcess = Start-Process msiexec.exe -ArgumentList "/i `"$bundledMsi`" /qn /norestart" -NoNewWindow -Wait -PassThru
        if ($msiProcess.ExitCode -eq 0 -or $msiProcess.ExitCode -eq 3010) {
            Write-Success "usbipd-win installed successfully from pre-bundled package."
        }
    } else {
        $wingetCmd = Get-Command "winget" -ErrorAction SilentlyContinue
        $installed = $false

        if ($wingetCmd) {
            try {
                Write-Host "Installing via winget..." -ForegroundColor Cyan
                Start-Process winget -ArgumentList "install --exact --id dorssel.usbipd-win --accept-source-agreements --accept-package-agreements --silent" -NoNewWindow -Wait
                $installed = $true
            } catch {}
        }

        if (-not $installed -or -not (Test-Path $defaultUsbipdPath)) {
            try {
                Write-Host "Downloading signed usbipd-win MSI directly from GitHub..." -ForegroundColor Cyan
                $downloadUrl = "https://github.com/dorssel/usbipd-win/releases/download/v5.3.0/usbipd-win_5.3.0_x64.msi"
                $tempMsi = "$env:TEMP\usbipd-win_installer.msi"
                [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
                Invoke-WebRequest -Uri $downloadUrl -OutFile $tempMsi -UseBasicParsing
                Start-Process msiexec.exe -ArgumentList "/i `"$tempMsi`" /qn /norestart" -NoNewWindow -Wait
                Remove-Item -Path $tempMsi -Force -ErrorAction SilentlyContinue
            } catch {
                Write-WarnMsg "Error during fallback download: $_"
            }
        }
    }

    # Refresh PATH in current session
    $env:Path = [System.Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path","User")
    if (Test-Path $defaultUsbipdPath -and $env:Path -notmatch "usbipd-win") {
        $env:Path = "$env:ProgramFiles\usbipd-win;" + $env:Path
    }
    $usbipdCmd = Get-Command "usbipd" -ErrorAction SilentlyContinue
}

if ($usbipdCmd -or (Test-Path $defaultUsbipdPath)) {
    Write-Success "'usbipd-win' verified and ready: $defaultUsbipdPath"
} else {
    Write-WarnMsg "Please install dorssel.usbipd-win manually from https://github.com/dorssel/usbipd-win/releases"
}

# 3. Configure Windows Firewall
Write-Step "Configuring Windows Defender Firewall rules..."
try {
    # Allow local USB/IP export port (TCP 3240)
    $existingFw3240 = Get-NetFirewallRule -DisplayName "ADBHUB-LINK Host USB/IP Export (TCP 3240)" -ErrorAction SilentlyContinue
    if (-not $existingFw3240) {
        New-NetFirewallRule -DisplayName "ADBHUB-LINK Host USB/IP Export (TCP 3240)" `
            -Direction Inbound `
            -Action Allow `
            -Protocol TCP `
            -LocalPort 3240 `
            -Description "Allows ADBHUB-LINK Agent to export local USB devices via signed usbipd-win" | Out-Null
        Write-Success "Inbound TCP 3240 firewall rule created."
    } else {
        Write-Success "Inbound TCP 3240 firewall rule already exists."
    }

    # Allow outbound QUIC relay (UDP 4433)
    $existingFw4433 = Get-NetFirewallRule -DisplayName "ADBHUB-LINK Outbound QUIC Relay (UDP 4433)" -ErrorAction SilentlyContinue
    if (-not $existingFw4433) {
        New-NetFirewallRule -DisplayName "ADBHUB-LINK Outbound QUIC Relay (UDP 4433)" `
            -Direction Outbound `
            -Action Allow `
            -Protocol UDP `
            -RemotePort 4433 `
            -Description "Allows ADBHUB-LINK Agent outbound encrypted QUIC tunnel to Cloud Relay" | Out-Null
        Write-Success "Outbound UDP 4433 firewall rule created."
    } else {
        Write-Success "Outbound UDP 4433 firewall rule already exists."
    }
} catch {
    Write-WarnMsg "Firewall configuration warning: $_"
}

# 4. Copy Binaries to Install Directory
Write-Step "Installing ADBHUB-LINK Agent binaries to '$InstallDir'..."
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

$sourceExe = "$PSScriptRoot\..\..\target\release\adbhub-agent.exe"
if (-not (Test-Path $sourceExe)) {
    $sourceExe = "$PSScriptRoot\..\..\target\debug\adbhub-agent.exe"
}

if (Test-Path $sourceExe) {
    Copy-Item -Path $sourceExe -Destination "$InstallDir\adbhub-agent.exe" -Force
    Write-Success "Binary copied: $InstallDir\adbhub-agent.exe"
} else {
    Write-WarnMsg "Compiled adbhub-agent.exe not found at default target location. Please build with 'cargo build --release' or place binary in $InstallDir."
}

# 5. Register Startup Task / Service
if (-not $NoService) {
    Write-Step "Registering ADBHUB-LINK Agent scheduled background task..."
    $taskName = "ADBHUB-LINK-Agent"
    $agentBin = "$InstallDir\adbhub-agent.exe"
    
    if (Test-Path $agentBin) {
        $action = New-ScheduledTaskAction -Execute $agentBin -Argument "watch --mobile-only"
        $trigger = New-ScheduledTaskTrigger -AtLogOn
        $principal = New-ScheduledTaskPrincipal -UserId "SYSTEM" -LogonType ServiceAccount -RunLevel Highest
        $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit 0 -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)

        try {
            Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
            Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings | Out-Null
            Write-Success "Scheduled Task '$taskName' registered to start at logon with SYSTEM privileges."
        } catch {
            Write-WarnMsg "Scheduled task creation failed: $_"
        }
    }
}

# 6. Run Doctor Diagnostic
Write-Step "Running Agent Doctor Diagnostic..."
$agentExec = "$InstallDir\adbhub-agent.exe"
if (Test-Path $agentExec) {
    & $agentExec doctor
} else {
    cargo run -p adbhub-agent -- doctor
}

Write-Host "`n================================================================================" -ForegroundColor Green
Write-Host " ADBHUB-LINK Agent installed successfully!" -ForegroundColor Green
Write-Host " To start monitoring in foreground: & '$InstallDir\adbhub-agent.exe' watch" -ForegroundColor Cyan
Write-Host "================================================================================`n" -ForegroundColor Green

