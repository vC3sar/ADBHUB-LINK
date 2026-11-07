# scripts/package_dist.ps1 - Generates portable distribution packages
param(
    [switch]$IncludeDesktop = $true
)

$ErrorActionPreference = "Continue"
$projectRoot = Split-Path -Parent $PSScriptRoot
$distDir = Join-Path $projectRoot "dist"
$tempStage = Join-Path $distDir "ADBHUB-LINK"

if (Test-Path $tempStage) {
    Remove-Item -Path $tempStage -Recurse -Force -ErrorAction SilentlyContinue
}
New-Item -ItemType Directory -Path $tempStage -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $tempStage "installer\packages") -Force | Out-Null

Write-Host ">>> Copiando binarios oficiales y controladores..." -ForegroundColor Cyan

Copy-Item (Join-Path $projectRoot "Setup.exe") $tempStage -Force
Copy-Item (Join-Path $projectRoot "adbhub-agent.exe") $tempStage -Force
Copy-Item (Join-Path $projectRoot "adbhub-desktop.exe") $tempStage -Force
Copy-Item (Join-Path $projectRoot "INICIAR-CLIENTE.bat") $tempStage -Force
Copy-Item (Join-Path $projectRoot "INICIAR-TECNICO.bat") $tempStage -Force
Copy-Item (Join-Path $projectRoot "MENU.bat") $tempStage -Force
Copy-Item (Join-Path $projectRoot "iniciar-cliente.sh") $tempStage -Force
Copy-Item (Join-Path $projectRoot "iniciar-tecnico.sh") $tempStage -Force
Copy-Item (Join-Path $projectRoot "install-linux.sh") $tempStage -Force
Copy-Item (Join-Path $projectRoot "adbhub-client.py") $tempStage -Force

# Create a Quick Readme
$readmeContent = @"
================================================================================
 ADBHUB-LINK - Modo Remoto ADB AnyDesk Edition (Windows & Linux / Lubuntu)
================================================================================
INSTRUCCIONES DE USO:

EN WINDOWS:
1. En la PC del CLIENTE: Abra 'INICIAR-CLIENTE.bat'.
   - Conecte el telefono Android por cable USB (Depuracion USB activa).
   - Vera su codigo AnyDesk (ej. A55A-DDD5-CCH6-RRTY). Entreguelo al tecnico.
2. En la PC del TECNICO: Abra 'INICIAR-TECNICO.bat'.
   - Escriba el codigo AnyDesk del cliente y pulse 'Conectar'.
   - Listo: Tiene acceso directo al telefono para enviar comandos y reiniciar.

EN LINUX (LUBUNTU / UBUNTU / DEBIAN):
1. Abra la terminal en esta carpeta.
2. Ejecute: chmod +x *.sh && ./install-linux.sh
3. Para Cliente: ./iniciar-cliente.sh
4. Para Tecnico: ./iniciar-tecnico.sh
================================================================================
"@
Set-Content -Path (Join-Path $tempStage "LEEME-INSTRUCCIONES.txt") -Value $readmeContent -Encoding utf8

$zipTarget = Join-Path $distDir "ADBHUB-LINK-Pack.zip"
if (Test-Path $zipTarget) {
    Remove-Item $zipTarget -Force -ErrorAction SilentlyContinue
}

Write-Host ">>> Creando archivo comprimido $zipTarget..." -ForegroundColor Cyan
Compress-Archive -Path "$tempStage\*" -DestinationPath $zipTarget -Force

$zipSizeMb = [math]::Round(((Get-Item $zipTarget).Length / 1MB), 2)
Write-Host "================================================================================" -ForegroundColor Green
Write-Host " [OK] Paquete generado exitosamente: $zipTarget ($zipSizeMb MB)" -ForegroundColor Green
Write-Host "================================================================================" -ForegroundColor Green

