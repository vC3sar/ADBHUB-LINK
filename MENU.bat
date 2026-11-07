@echo off
title ADBHUB-LINK - Menu Principal
cd /d "%~dp0"
:menu
cls
echo ================================================================================
echo  ADBHUB-LINK v0.2.0 - Centro de Control y Acceso Rapido
echo ================================================================================
echo.
echo  [1] Iniciar Estacion del Tecnico (adbhub-desktop.exe)
echo  [2] Iniciar Estacion del Cliente (adbhub-agent.exe)
echo  [3] Ejecutar Instalador Oficial (Setup.exe con drivers silenciosos)
echo  [4] Compartir / Servir Paquete para Segunda PC (Cliente)
echo  [5] Salir
echo.
echo ================================================================================
choice /c 12345 /n /m "Seleccione una opcion [1-5]: "

if errorlevel 5 exit
if errorlevel 4 goto compartir
if errorlevel 3 goto setup
if errorlevel 2 goto cliente
if errorlevel 1 goto tecnico
goto menu

:tecnico
start "" "adbhub-desktop.exe"
goto menu

:cliente
start "" "adbhub-agent.exe"
goto menu

:setup
start "" "Setup.exe"
goto menu

:compartir
call "SUBIR-A-SEGUNDA-PC.bat"
goto menu
