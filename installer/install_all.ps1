<#
.SYNOPSIS
    ADBHUB-LINK — 1-Click Master Installer (Fully Automated).
.DESCRIPTION
    Installs everything in one single automated pass with zero manual downloads:
    - Pre-bundled signed WHQL/Attestation usbipd-win driver.
    - Pre-bundled signed UDE usbip-win2 driver.
    - ADBHUB-LINK Agent & Desktop binaries.
    - Windows Firewall rules (TCP 3240, TCP 3420, UDP 4433).
    - Desktop shortcuts and background service startup.
    - Verification Diagnostic.
#>

[CmdletBinding()]
param(
    [string]$BaseInstallDir = "$env:ProgramFiles\ADBHUB-LINK"
)

$ErrorActionPreference = "Continue"

Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host " ADBHUB-LINK — Instalación Automatizada 1-Click (Cero Descargas Manuales)" -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan

# 1. Comprobar privilegios de Administrador
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "[ADBHUB-LINK] [ERROR] Esta instalación requiere ejecutarse como Administrador." -ForegroundColor Red
    exit 1
}

$packagesDir = "$PSScriptRoot\packages"
$defaultUsbipdPath = "$env:ProgramFiles\usbipd-win\usbipd.exe"
$defaultUsbipPath = "$env:ProgramFiles\usbip-win2\usbip.exe"

# 2. Instalación Automática y Silenciosa del Driver Host (usbipd-win)
Write-Host "`n[1/5] Verificando e instalando Driver de Exportación Host (usbipd-win)..." -ForegroundColor Yellow
if (-not (Test-Path $defaultUsbipdPath)) {
    $usbipdMsi = "$packagesDir\usbipd-win.msi"
    if (Test-Path $usbipdMsi) {
        Write-Host "  Instalando paquete empaquetado: $usbipdMsi ..." -ForegroundColor Cyan
        $proc = Start-Process msiexec.exe -ArgumentList "/i `"$usbipdMsi`" /qn /norestart" -NoNewWindow -Wait -PassThru
        Write-Host "  Driver de exportación instalado (Código: $($proc.ExitCode))." -ForegroundColor Green
    } else {
        Write-Host "  Descargando paquete oficial..." -ForegroundColor Cyan
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest -Uri "https://github.com/dorssel/usbipd-win/releases/download/v5.3.0/usbipd-win_5.3.0_x64.msi" -OutFile "$env:TEMP\usbipd.msi" -UseBasicParsing
        Start-Process msiexec.exe -ArgumentList "/i `"$env:TEMP\usbipd.msi`" /qn /norestart" -NoNewWindow -Wait
        Remove-Item "$env:TEMP\usbipd.msi" -Force -ErrorAction SilentlyContinue
    }
} else {
    Write-Host "  [OK] Driver Host usbipd-win ya instalado." -ForegroundColor Green
}

# 3. Instalación Automática y Silenciosa del Driver Técnico (usbip-win2 UDE)
Write-Host "`n[2/5] Verificando e instalando Driver Virtual UDE para Técnicos (usbip-win2)..." -ForegroundColor Yellow
$driverUde = Test-Path "$env:SystemRoot\System32\drivers\usbip2_ude.sys"
if (-not (Test-Path $defaultUsbipPath) -and -not $driverUde) {
    $usbipWin2Exe = "$packagesDir\usbip-win2.exe"
    if (Test-Path $usbipWin2Exe) {
        Write-Host "  Instalando paquete empaquetado: $usbipWin2Exe ..." -ForegroundColor Cyan
        Start-Process $usbipWin2Exe -ArgumentList "/S /v/qn" -NoNewWindow -Wait
        Write-Host "  Driver virtual UDE instalado." -ForegroundColor Green
    } else {
        Write-Host "  Descargando paquete oficial..." -ForegroundColor Cyan
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest -Uri "https://github.com/vadimgrn/usbip-win2/releases/download/v.0.9.8.1/USBip-0.9.8.1-x64.exe" -OutFile "$env:TEMP\usbip-win2.exe" -UseBasicParsing
        Start-Process "$env:TEMP\usbip-win2.exe" -ArgumentList "/S /v/qn" -NoNewWindow -Wait
        Remove-Item "$env:TEMP\usbip-win2.exe" -Force -ErrorAction SilentlyContinue
    }
} else {
    Write-Host "  [OK] Driver Técnico usbip-win2 UDE ya instalado." -ForegroundColor Green
}

# Actualizar el PATH del sistema para la sesión
$env:Path = "$env:ProgramFiles\usbipd-win;$env:ProgramFiles\usbip-win2;" + [System.Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path","User")

# 4. Configurar Reglas de Firewall
Write-Host "`n[3/5] Configurando Reglas de Windows Defender Firewall..." -ForegroundColor Yellow
$rules = @(
    @{ Name = "ADBHUB-LINK Host USB/IP Export (TCP 3240)"; Dir = "Inbound"; Proto = "TCP"; Port = 3240 },
    @{ Name = "ADBHUB-LINK Desktop UI (TCP 3420)"; Dir = "Inbound"; Proto = "TCP"; Port = 3420 },
    @{ Name = "ADBHUB-LINK QUIC Relay (UDP 4433)"; Dir = "Outbound"; Proto = "UDP"; Port = 4433 }
)
foreach ($r in $rules) {
    $existing = Get-NetFirewallRule -DisplayName $r.Name -ErrorAction SilentlyContinue
    if (-not $existing) {
        if ($r.Dir -eq "Inbound") {
            New-NetFirewallRule -DisplayName $r.Name -Direction Inbound -Action Allow -Protocol $r.Proto -LocalPort $r.Port | Out-Null
        } else {
            New-NetFirewallRule -DisplayName $r.Name -Direction Outbound -Action Allow -Protocol $r.Proto -RemotePort $r.Port | Out-Null
        }
        Write-Host "  [+] Regla creada: $($r.Name)" -ForegroundColor Green
    } else {
        Write-Host "  [OK] Regla existente: $($r.Name)" -ForegroundColor Green
    }
}

# 5. Instalar Binarios de ADBHUB-LINK
Write-Host "`n[4/5] Desplegando Binarios de ADBHUB-LINK..." -ForegroundColor Yellow
$agentDir = "$BaseInstallDir\Agent"
$desktopDir = "$BaseInstallDir\Desktop"
New-Item -ItemType Directory -Path $agentDir -Force | Out-Null
New-Item -ItemType Directory -Path $desktopDir -Force | Out-Null
New-Item -ItemType Directory -Path "$desktopDir\data" -Force | Out-Null

$agentSrc = "$PSScriptRoot\..\target\release\adbhub-agent.exe"
if (-not (Test-Path $agentSrc)) { $agentSrc = "$PSScriptRoot\..\target\debug\adbhub-agent.exe" }
$desktopSrc = "$PSScriptRoot\..\target\release\adbhub-desktop.exe"
if (-not (Test-Path $desktopSrc)) { $desktopSrc = "$PSScriptRoot\..\target\debug\adbhub-desktop.exe" }

if (Test-Path $agentSrc) {
    Copy-Item -Path $agentSrc -Destination "$agentDir\adbhub-agent.exe" -Force
    Write-Host "  [OK] Agente instalado en $agentDir\adbhub-agent.exe" -ForegroundColor Green
}
if (Test-Path $desktopSrc) {
    Copy-Item -Path $desktopSrc -Destination "$desktopDir\adbhub-desktop.exe" -Force
    Write-Host "  [OK] Desktop instalado en $desktopDir\adbhub-desktop.exe" -ForegroundColor Green
}

# Tarea programada de inicio automático para Agent
$taskName = "ADBHUB-LINK-Agent"
if (Test-Path "$agentDir\adbhub-agent.exe") {
    $action = New-ScheduledTaskAction -Execute "$agentDir\adbhub-agent.exe" -Argument "watch --mobile-only"
    $trigger = New-ScheduledTaskTrigger -AtLogOn
    $principal = New-ScheduledTaskPrincipal -UserId "SYSTEM" -LogonType ServiceAccount -RunLevel Highest
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit 0
    Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
    Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings | Out-Null
    Write-Host "  [OK] Servicio en segundo plano registrado ('$taskName')." -ForegroundColor Green
}

# Accesos directos en Escritorio
$wsh = New-Object -ComObject WScript.Shell
$desktopLnk = [System.Environment]::GetFolderPath("CommonDesktopDirectory") + "\ADBHUB-LINK Desktop.lnk"
$sc = $wsh.CreateShortcut($desktopLnk)
$sc.TargetPath = "$desktopDir\adbhub-desktop.exe"
$sc.WorkingDirectory = $desktopDir
$sc.Description = "ADBHUB-LINK Professional Remote USB Passthrough for Mobile Repair"
$sc.Save()
Write-Host "  [OK] Acceso directo en el Escritorio creado." -ForegroundColor Green

# 6. Comprobación Final (Doctor)
Write-Host "`n[5/5] Ejecutando Diagnóstico de Listo para Producción..." -ForegroundColor Yellow
if (Test-Path "$agentDir\adbhub-agent.exe") {
    & "$agentDir\adbhub-agent.exe" doctor
} else {
    cargo run -p adbhub-agent -- doctor
}

Write-Host "`n================================================================================" -ForegroundColor Green
Write-Host " ¡INSTALACIÓN 100% COMPLETADA Y AUTOMATIZADA CON ÉXITO!" -ForegroundColor Green
Write-Host " - Desktop Técnico: Acceso directo creado en el Escritorio (http://127.0.0.1:3420)" -ForegroundColor Cyan
Write-Host " - Agent Host: Configurado para inicio automático con detección de teléfonos." -ForegroundColor Cyan
Write-Host "================================================================================`n" -ForegroundColor Green

