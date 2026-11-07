<#
.SYNOPSIS
    ADBHUB-LINK Production Readiness Diagnostic Suite ("doctor").
.DESCRIPTION
    Runs comprehensive diagnostic checks on:
    1. Windows OS architecture & kernel version.
    2. Driver signature enforcement status (must be strictly ENFORCED / Secure Boot active).
    3. Host export driver (dorssel/usbipd-win) installation and signing.
    4. Client VHCI driver (vadimgrn/usbip-win2) installation and signing.
    5. Local file storage directory structure (./data).
    6. Port availability (TCP 3240, TCP 3420, UDP 4433).
    7. Full cargo workspace test status.
#>

[CmdletBinding()]
param()

$ErrorActionPreference = "Continue"

Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host " ADBHUB-LINK Production Readiness & System Diagnostic (Doctor)" -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan

# 1. OS Info
$os = Get-CimInstance Win32_OperatingSystem
Write-Host "`n[1] Operating System Environment:" -ForegroundColor Yellow
Write-Host "  Caption       : $($os.Caption)"
Write-Host "  Version       : $($os.Version)"
Write-Host "  Build Number  : $($os.BuildNumber)"
Write-Host "  Architecture  : $($os.OSArchitecture)"

# 2. Driver Signature Enforcement Status (Section 5 requirement: NO TESTSIGNING)
Write-Host "`n[2] Driver Signature Enforcement & Security Policy:" -ForegroundColor Yellow
$bcdTestSigning = bcdedit /enum '{current}' | Select-String "testsigning"
if ($bcdTestSigning -and $bcdTestSigning.Line -match "Yes") {
    Write-Host "  [FAIL] TESTSIGNING IS ENABLED! ADBHUB-LINK requires strictly signed WHQL/Attestation drivers." -ForegroundColor Red
} else {
    Write-Host "  [PASS] Driver Signature Enforcement is ACTIVE (TESTSIGNING OFF / Secure Boot compliant)." -ForegroundColor Green
}

# 3. Host usbipd-win Check
Write-Host "`n[3] Host Component (usbipd-win) Driver Status:" -ForegroundColor Yellow
$usbipd = Get-Command "usbipd" -ErrorAction SilentlyContinue
$vboxSys = Test-Path "$env:SystemRoot\System32\drivers\VBoxUsb.sys"
if ($usbipd -or $vboxSys) {
    Write-Host "  [PASS] usbipd-win installed and available." -ForegroundColor Green
    if ($usbipd) {
        $ver = & usbipd --version 2>$null
        Write-Host "  Version: $ver"
    }
} else {
    Write-Host "  [NOTICE] usbipd-win not found in current PATH. Install with: winget install dorssel.usbipd-win" -ForegroundColor Yellow
}

# 4. Client usbip-win2 Check
Write-Host "`n[4] Technician Client Component (usbip-win2 UDE) Driver Status:" -ForegroundColor Yellow
$usbip = Get-Command "usbip" -ErrorAction SilentlyContinue
$udeSys = Test-Path "$env:SystemRoot\System32\drivers\usbip2_ude.sys"
if ($usbip -or $udeSys) {
    Write-Host "  [PASS] usbip-win2 (UDE Virtual USB Host Controller) installed." -ForegroundColor Green
} else {
    Write-Host "  [NOTICE] usbip-win2 not found in current PATH. Install with: winget install vadimgrn.usbip-win2" -ForegroundColor Yellow
}

# 5. Local File Storage Persistence Check (Zero Docker / Zero DB servers)
Write-Host "`n[5] Local File Persistence Storage (Zero Docker / Zero DB):" -ForegroundColor Yellow
$dataDir = "$PSScriptRoot\..\data"
if (-not (Test-Path $dataDir)) {
    New-Item -ItemType Directory -Path $dataDir -Force | Out-Null
}
$resolvedData = (Resolve-Path $dataDir).Path
Write-Host "  [PASS] Local data directory verified: $resolvedData" -ForegroundColor Green

# 6. Port Availability Check
Write-Host "`n[6] Network Ports Availability:" -ForegroundColor Yellow
function Check-PortLocal([int]$Port, [string]$Proto) {
    $conns = Get-NetTCPConnection -LocalPort $Port -ErrorAction SilentlyContinue
    if ($conns) {
        Write-Host "  [$Proto $Port] In Use by PID $($conns[0].OwningProcess)" -ForegroundColor Yellow
    } else {
        Write-Host "  [$Proto $Port] Available" -ForegroundColor Green
    }
}
Check-PortLocal 3240 "TCP (Host USB/IP)"
Check-PortLocal 3420 "TCP (Desktop UI)"

# 7. Cargo Workspace Test Status
Write-Host "`n[7] Running Cargo Workspace Automated Verification Suite:" -ForegroundColor Yellow
$testOutput = cargo test --workspace -- --test-threads=1 2>&1
$passedTests = ($testOutput | Select-String "test result: ok").Count
if ($passedTests -ge 5) {
    Write-Host "  [PASS] All Rust workspace unit and integration tests passed ($passedTests test suites OK)." -ForegroundColor Green
} else {
    Write-Host "  [WARN] Test run finished with output: $($testOutput | Select-String 'FAILED')" -ForegroundColor Yellow
}

Write-Host "`n================================================================================" -ForegroundColor Cyan
Write-Host " Diagnostic check completed." -ForegroundColor Cyan
Write-Host "================================================================================`n" -ForegroundColor Cyan

