@echo off
:: ==============================================================================
:: ADBHUB-LINK — Instalador Automatizado 1-Click con Auto-Elevación de Privilegios
:: ==============================================================================
TITLE ADBHUB-LINK Automated Installer

:: Comprobar privilegios de Administrador
net session >nul 2>&1
if %errorLevel% == 0 (
    goto :RunInstall
) else (
    echo [ADBHUB-LINK] Solicitando permisos de Administrador para configurar drivers y firewall...
    powershell -Command "Start-Process cmd -ArgumentList '/c \"%~dp0install.bat\"' -Verb RunAs"
    exit /b
)

:RunInstall
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0installer\install_all.ps1"
echo.
echo Presione cualquier tecla para salir...
pause >nul

