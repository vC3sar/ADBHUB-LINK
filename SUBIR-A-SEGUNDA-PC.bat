@echo off
title ADBHUB-LINK - Compartir con Segunda PC
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "scripts\share_pack.ps1"
pause

