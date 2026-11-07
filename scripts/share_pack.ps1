# scripts/share_pack.ps1 - Servidor de Descarga Multiplataforma (Windows y Linux)
param(
    [int]$Port = 8888
)

$ErrorActionPreference = "Continue"
$projectRoot = Split-Path -Parent $PSScriptRoot
$distDir = Join-Path $projectRoot "dist"
$zipPath = Join-Path $distDir "ADBHUB-LINK-Pack.zip"
$linuxInstallSh = Join-Path $projectRoot "install-linux.sh"
$linuxClientSh = Join-Path $projectRoot "iniciar-cliente.sh"

if (-not (Test-Path $zipPath)) {
    Write-Host ">>> Generando paquete comprimido..." -ForegroundColor Cyan
    & (Join-Path $projectRoot "scripts\package_dist.ps1")
}

$localIps = Get-NetIPAddress -AddressFamily IPv4 -InterfaceAlias "Wi-Fi*", "Ethernet*" -ErrorAction SilentlyContinue |
    Where-Object { $_.IPAddress -notlike "127.*" -and $_.IPAddress -notlike "169.254.*" }

$primaryIp = if ($localIps) { $localIps[0].IPAddress } else { "127.0.0.1" }
$zipSizeMb = if (Test-Path $zipPath) { [math]::Round(((Get-Item $zipPath).Length / 1MB), 2) } else { 0 }

Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host " ADBHUB-LINK - Servidor de Descarga (Windows y Linux / Lubuntu)" -ForegroundColor Yellow
Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host " Archivo Windows/Linux: $zipPath ($zipSizeMb MB)"
Write-Host "--------------------------------------------------------------------------------"
Write-Host " [1] Iniciar Servidor Web de Descarga (HTTP Directo - Sin Error 400)"
Write-Host " [2] Subir a Servidor Temporal en la Nube (file.io / oshi.at)"
Write-Host " [3] Copiar al Escritorio"
Write-Host "================================================================================"

$choice = Read-Host " Seleccione una opcion [1-3] (Por defecto 1)"
if ([string]::IsNullOrWhiteSpace($choice)) { $choice = "1" }

if ($choice -eq "1") {
    Write-Host ""
    Write-Host ">>> Servidor HTTP TCP Socket activo en 0.0.0.0:$Port" -ForegroundColor Green
    Write-Host "--------------------------------------------------------------------------------"
    Write-Host " EN CUALQUIER OTRA PC (WINDOWS O LUBUNTU / LINUX):" -ForegroundColor Yellow
    Write-Host " Abra el navegador web y acceda a cualquiera de estas direcciones:" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "    Panel Web Principal:   http://${primaryIp}:${Port}/" -ForegroundColor White -BackgroundColor DarkBlue
    Write-Host "    Descarga Windows:      http://${primaryIp}:${Port}/download/windows" -ForegroundColor Cyan
    Write-Host "    Descarga Linux:        http://${primaryIp}:${Port}/download/linux" -ForegroundColor Cyan
    Write-Host ""
    Write-Host " Presione Ctrl+C en esta ventana para detener el servidor cuando termine."
    Write-Host "--------------------------------------------------------------------------------"

    # Raw TcpListener to bypass Windows http.sys 'Invalid Hostname / Error 400'
    $endpoint = New-Object System.Net.IPEndPoint([System.Net.IPAddress]::Any, $Port)
    $tcpListener = New-Object System.Net.Sockets.TcpListener($endpoint)
    try {
        $tcpListener.Start()
    } catch {
        Write-Host "[ERROR] No se pudo abrir el puerto $Port. Asegurese de que no este ocupado." -ForegroundColor Red
        return
    }

    while ($true) {
        $client = $tcpListener.AcceptTcpClient()
        $stream = $client.GetStream()
        $reader = New-Object System.IO.StreamReader($stream)
        $requestLine = $reader.ReadLine()

        if ([string]::IsNullOrWhiteSpace($requestLine)) {
            $client.Close()
            continue
        }

        $remoteIp = $client.Client.RemoteEndPoint.ToString()
        Write-Host "[PETICION] $requestLine desde $remoteIp" -ForegroundColor Cyan

        # Determine route
        $path = ""
        $parts = $requestLine.Split(" ")
        if ($parts.Length -ge 2) {
            $path = $parts[1]
        }

        if ($path -eq "/adbhub-client.py") {
            $pyPath = Join-Path $projectRoot "adbhub-client.py"
            if (Test-Path $pyPath) {
                $bytes = [System.IO.File]::ReadAllBytes($pyPath)
                $header = "HTTP/1.1 200 OK`r`nContent-Type: text/x-python`r`nContent-Disposition: attachment; filename=adbhub-client.py`r`nContent-Length: $($bytes.Length)`r`nConnection: close`r`n`r`n"
                $headerBytes = [System.Text.Encoding]::ASCII.GetBytes($header)
                $stream.Write($headerBytes, 0, $headerBytes.Length)
                $stream.Write($bytes, 0, $bytes.Length)
                Write-Host "[OK] adbhub-client.py enviado con exito." -ForegroundColor Green
            }
        }
        elseif ($path -eq "/iniciar-cliente.sh") {
            $shPath = Join-Path $projectRoot "iniciar-cliente.sh"
            if (Test-Path $shPath) {
                $bytes = [System.IO.File]::ReadAllBytes($shPath)
                $header = "HTTP/1.1 200 OK`r`nContent-Type: text/x-shellscript`r`nContent-Disposition: attachment; filename=iniciar-cliente.sh`r`nContent-Length: $($bytes.Length)`r`nConnection: close`r`n`r`n"
                $headerBytes = [System.Text.Encoding]::ASCII.GetBytes($header)
                $stream.Write($headerBytes, 0, $headerBytes.Length)
                $stream.Write($bytes, 0, $bytes.Length)
                Write-Host "[OK] iniciar-cliente.sh enviado con exito." -ForegroundColor Green
            }
        }
        elseif ($path -eq "/download/linux" -or $path -eq "/install-linux.sh" -or $path -eq "/linux") {
            # Serve Linux Installer
            if (Test-Path $linuxInstallSh) {
                $bytes = [System.IO.File]::ReadAllBytes($linuxInstallSh)
                $header = "HTTP/1.1 200 OK`r`nContent-Type: text/x-shellscript`r`nContent-Disposition: attachment; filename=install-linux.sh`r`nContent-Length: $($bytes.Length)`r`nConnection: close`r`n`r`n"
                $headerBytes = [System.Text.Encoding]::ASCII.GetBytes($header)
                $stream.Write($headerBytes, 0, $headerBytes.Length)
                $stream.Write($bytes, 0, $bytes.Length)
                Write-Host "[OK] Script Linux enviado con exito." -ForegroundColor Green
            }
        }
        elseif ($path -eq "/download/windows" -or $path -eq "/download" -or $path.EndsWith(".zip")) {
            # Serve Full Portable Zip
            if (Test-Path $zipPath) {
                $bytes = [System.IO.File]::ReadAllBytes($zipPath)
                $header = "HTTP/1.1 200 OK`r`nContent-Type: application/zip`r`nContent-Disposition: attachment; filename=ADBHUB-LINK-Pack.zip`r`nContent-Length: $($bytes.Length)`r`nConnection: close`r`n`r`n"
                $headerBytes = [System.Text.Encoding]::ASCII.GetBytes($header)
                $stream.Write($headerBytes, 0, $headerBytes.Length)
                $stream.Write($bytes, 0, $bytes.Length)
                Write-Host "[OK] Paquete ZIP ($zipSizeMb MB) enviado con exito." -ForegroundColor Green
            }
        }
        else {
            # Serve Landing Page with Download Buttons
            $html = @"
<!DOCTYPE html>
<html lang="es">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>ADBHUB-LINK - Centro de Descargas</title>
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #0f172a; color: #f8fafc; text-align: center; padding: 40px 20px; }
        .card { background: #1e293b; max-width: 540px; margin: 0 auto; padding: 30px; border-radius: 16px; box-shadow: 0 10px 25px rgba(0,0,0,0.5); border: 1px solid #334155; }
        h1 { color: #38bdf8; margin-bottom: 8px; font-size: 26px; }
        p { color: #94a3b8; font-size: 15px; margin-bottom: 25px; }
        .btn { display: block; padding: 16px 20px; margin: 12px 0; border-radius: 10px; font-weight: bold; font-size: 16px; text-decoration: none; transition: 0.2s; }
        .btn-win { background: #0284c7; color: #fff; }
        .btn-win:hover { background: #0369a1; }
        .btn-linux { background: #16a34a; color: #fff; }
        .btn-linux:hover { background: #15803d; }
        .footer { margin-top: 25px; font-size: 12px; color: #64748b; }
    </style>
</head>
<body>
    <div class="card">
        <h1>ADBHUB-LINK</h1>
        <p>Seleccione su sistema operativo para descargar el cliente:</p>
        <a href="/download/windows" class="btn btn-win">Descargar para Windows (.ZIP - $zipSizeMb MB)</a>
        <a href="/download/windows" class="btn btn-linux">Descargar para Lubuntu / Linux (.ZIP Completo)</a>
        <a href="/download/linux" class="btn" style="background:#475569; color:#f1f5f9; font-size:14px;">Descargar Script Linux (install-linux.sh)</a>
        <div class="footer">ADBHUB-LINK AnyDesk Edition - Cero Drivers Kernel Requeridos</div>
    </div>
</body>
</html>
"@
            $htmlBytes = [System.Text.Encoding]::UTF8.GetBytes($html)
            $header = "HTTP/1.1 200 OK`r`nContent-Type: text/html; charset=utf-8`r`nContent-Length: $($htmlBytes.Length)`r`nConnection: close`r`n`r`n"
            $headerBytes = [System.Text.Encoding]::ASCII.GetBytes($header)
            $stream.Write($headerBytes, 0, $headerBytes.Length)
            $stream.Write($htmlBytes, 0, $htmlBytes.Length)
        }

        $stream.Flush()
        $stream.Close()
        $client.Close()
    }
}
elseif ($choice -eq "2") {
    Write-Host ""
    Write-Host ">>> Subiendo a servidor temporal en la nube..." -ForegroundColor Cyan
    & curl.exe -s -F "file=@$zipPath" https://file.io
    Write-Host ""
    & curl.exe -T "$zipPath" https://oshi.at
}
elseif ($choice -eq "3") {
    $desktopPath = [Environment]::GetFolderPath("Desktop")
    $dest = Join-Path $desktopPath "ADBHUB-LINK-Pack.zip"
    Copy-Item $zipPath $dest -Force
    Write-Host ""
    Write-Host "[OK] Archivo copiado al Escritorio: $dest" -ForegroundColor Green
}
