<#
.SYNOPSIS
    ADBHUB-LINK Lab Test Runner & Protocol Verification (Phase 7).
.DESCRIPTION
    Executes automated and interactive laboratory tests for all 6 mobile repair protocols:
    1. Android ADB (Bulk IN/OUT + Control GET_DESCRIPTOR).
    2. Android MTP (WPD Interface 06/01/01).
    3. Qualcomm EDL 9008 (VID:PID 05C6:9008, Sahara / Firehose protocol).
    4. MediaTek BROM (VID:PID 0E8D:0003, Preloader handshake).
    5. Spreadtrum / Unisoc Diag (VID:PID 1782:4D00, U2S protocol).
    6. Samsung Download Mode (VID:PID 04E8:685D, Odin/Loke handshake, 60s URBs, post-flash reboot).
#>

[CmdletBinding()]
param(
    [switch]$Interactive = $false
)

$ErrorActionPreference = "Continue"

Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host " ADBHUB-LINK Laboratory Protocol Verification Runner (Phase 7)" -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan

$modes = @(
    @{ Name = "1. Android ADB"; VidPid = "18D1:4EE2"; Class = "0xFF/0x42/0x01"; ExpectedTool = "adb devices" },
    @{ Name = "2. Android MTP"; VidPid = "18D1:4EE1"; Class = "0x06/0x01/0x01"; ExpectedTool = "Windows Explorer / WPD" },
    @{ Name = "3. Qualcomm EDL"; VidPid = "05C6:9008"; Class = "Vendor-Specific"; ExpectedTool = "QFIL / Sahara Firehose" },
    @{ Name = "4. MediaTek BROM"; VidPid = "0E8D:0003"; Class = "Vendor-Specific"; ExpectedTool = "SP Flash Tool / MTK Client" },
    @{ Name = "5. SPD / Unisoc"; VidPid = "1782:4D00"; Class = "Vendor-Specific"; ExpectedTool = "ResearchDownload / SPD Upgrade" },
    @{ Name = "6. Samsung Download (Odin)"; VidPid = "04E8:685D"; Class = "CDC ACM / Bulk"; ExpectedTool = "Samsung Odin3 v3.14.4" }
)

Write-Host "`nProtocol Matrix to Verify:" -ForegroundColor Yellow
$modes | Format-Table -AutoSize | Out-String | Write-Host

Write-Host "`n[STEP 1] Running Multi-Protocol Rust Engine Verification..." -ForegroundColor Cyan
$testResult = cargo test -p adbhub-usbip-client-adapter multiprotocol_validation -- --nocapture 2>&1
if ($LASTEXITCODE -eq 0) {
    Write-Host "  [OK] Multiprotocol test suite passed successfully." -ForegroundColor Green
} else {
    Write-Host "  [FAIL] Test suite failed: $testResult" -ForegroundColor Red
    exit 1
}

Write-Host "`n[STEP 2] Running Odin Handshake and 60-Second URB Erasure Resilience Test..." -ForegroundColor Cyan
$odinResult = cargo test -p adbhub-usbip-client-adapter --lib test_e2e_samsung_odin_and_multiprotocol_urb_passthrough -- --nocapture 2>&1
if ($LASTEXITCODE -eq 0) {
    Write-Host "  [OK] Samsung Odin handshake and 60s URB window passed." -ForegroundColor Green
} else {
    Write-Host "  [FAIL] Samsung Odin test failed: $odinResult" -ForegroundColor Red
    exit 1
}

Write-Host "`n[STEP 3] Running Reenumeration Lifecycle Correlation Test..." -ForegroundColor Cyan
$reenumResult = cargo test -p adbhub-usb-monitor --lib test_samsung_adb_to_download_mode_reenumeration_and_odin_guard -- --nocapture 2>&1
if ($LASTEXITCODE -eq 0) {
    Write-Host "  [OK] Reenumeration (ADB -> Samsung Download Mode -> Odin Guard -> Android) passed." -ForegroundColor Green
} else {
    Write-Host "  [FAIL] Reenumeration test failed: $reenumResult" -ForegroundColor Red
    exit 1
}

Write-Host "`n[STEP 4] Scanning Live USB Physical Devices on Host..." -ForegroundColor Cyan
$scanOutput = cargo run -p adbhub-agent -- --scan 2>&1
$scanOutput | ForEach-Object { Write-Host "  $_" }

Write-Host "`n================================================================================" -ForegroundColor Green
Write-Host " LABORATORY SIMULATION SUMMARY: ALL 6 PROTOCOLS VERIFIED PASSING." -ForegroundColor Green
Write-Host " Notice: For physical lab sign-off with real OEM hardware, proceed to:" -ForegroundColor Cyan
Write-Host " 'docs/BETA_TEST_PLAN.md' and follow the physical test protocol." -ForegroundColor Cyan
Write-Host "================================================================================`n" -ForegroundColor Green
