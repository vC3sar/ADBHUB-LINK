# ADBHUB-LINK — Guía de Despliegue en Producción e Instaladores

Esta guía describe el procedimiento de instalación, empaquetado y puesta en producción de **ADBHUB-LINK** tanto en la computadora anfitriona del cliente/taller (Host PC) como en la estación de trabajo del técnico (Technician PC).

---

## 1. Arquitectura de Despliegue y Prerrequisitos

ADBHUB-LINK opera sin Docker ni servidores externos de base de datos; todo el estado se mantiene localmente en archivos JSON transaccionales (`./data/`).

```text
+-----------------------------+                  +-----------------------------+
|      HOST PC (CLIENTE)      |                  |   TECHNICIAN PC (TÉCNICO)   |
|                             |                  |                             |
|  [Físico] Teléfono Móvil    |                  |  Samsung Odin / QFIL / ADB  |
|            |                |                  |            |                |
|       dorssel/usbipd-win    |                  |     vadimgrn/usbip-win2     |
|       (Signed Filter Driver)|                  |     (Signed UDE Driver)     |
|            |                |                  |            |                |
|     ADBHUB-LINK Agent       |== QUIC Relay ===>|    ADBHUB-LINK Desktop      |
|  (Background Service/Task)  | (UDP:4433 TLS1.3)|  (Web GUI en TCP:3420)      |
+-----------------------------+                  +-----------------------------+
```

### Prerrequisitos de Drivers Firmados de Windows

> [!IMPORTANT]
> **Estricto cumplimiento de seguridad:** ADBHUB-LINK **NO** requiere ni permite desactivar la firma de drivers (`TESTSIGNING ON` está estrictamente prohibido). Ambos componentes utilizan drivers kernel firmados compatibles con Secure Boot:
> - **Host PC:** `dorssel/usbipd-win` (Firma WHQL / Attestation de Microsoft, driver `VBoxUsb.sys`).
> - **Technician PC:** `vadimgrn/usbip-win2` (Firma Open Source Codesigning Initiative, driver `usbip2_ude.sys`).

---

## 2. Instalación en la PC del Host (Cliente / Taller)

### Opción A: Script Automatizado PowerShell (`installer/agent/install-agent.ps1`)

Ejecutar PowerShell como Administrador:

```powershell
Set-ExecutionPolicy Bypass -Scope Process
.\installer\agent\install-agent.ps1
```

Este script realiza automáticamente:
1. Comprobación de permisos de Administrador.
2. Instalación de `usbipd-win` mediante `winget install dorssel.usbipd-win`.
3. Configuración de reglas de firewall:
   - Inbound TCP `3240` (Exportación USB/IP local).
   - Outbound UDP `4433` (Túnel QUIC seguro hacia el Relay).
4. Instalación del binario en `C:\Program Files\ADBHUB-LINK\Agent\adbhub-agent.exe`.
5. Registro de la Tarea Programada de Windows `ADBHUB-LINK-Agent` con privilegios `SYSTEM` al iniciar sesión.
6. Ejecución del diagnóstico `adbhub-agent doctor`.

### Opción B: Comandos Manuales

```powershell
# 1. Instalar usbipd-win
winget install --exact --id dorssel.usbipd-win --accept-source-agreements --accept-package-agreements

# 2. Abrir puertos en Firewall
netsh advfirewall firewall add rule name="ADBHUB-LINK Host USB/IP" dir=in action=allow protocol=TCP localport=3240
netsh advfirewall firewall add rule name="ADBHUB-LINK QUIC Relay" dir=out action=allow protocol=UDP remoteport=4433

# 3. Iniciar el agente en modo monitorización continua
adbhub-agent.exe watch --mobile-only
```

---

## 3. Instalación en la PC del Técnico (Technician Workstation)

### Opción A: Script Automatizado PowerShell (`installer/desktop/install-desktop.ps1`)

Ejecutar PowerShell como Administrador:

```powershell
Set-ExecutionPolicy Bypass -Scope Process
.\installer\desktop\install-desktop.ps1
```

Este script realiza:
1. Verificación de permisos de Administrador.
2. Instalación del driver virtual UDE `vadimgrn.usbip-win2`.
3. Configuración de reglas de firewall locales:
   - Inbound TCP `3420` (Interfaz de usuario local de Desktop).
   - Outbound UDP `4433` (Túnel QUIC de transporte URB).
4. Copia de binarios a `C:\Program Files\ADBHUB-LINK\Desktop\`.
5. Creación de accesos directos en el Escritorio y Menú Inicio.

### Opción B: Instalador Gráfico Unificado (Inno Setup 6)

Para empaquetar un instalador `.exe` único distribuible comercialmente:

1. Abrir `installer/InnoSetup/adbhub-link-installer.iss` con Inno Setup Compiler.
2. Hacer clic en **Build -> Compile**.
3. El instalador generado `dist/ADBHUB-LINK-Setup-v0.2.0.exe` permite al usuario seleccionar:
   - **Estación del Técnico:** Instala Desktop UI y driver virtual UDE.
   - **PC del Cliente / Taller:** Instala Agent y driver exportador usbipd.
   - **Instalación Completa:** Para entornos de desarrollo o pruebas locales.

---

## 4. Diagnóstico de Sistema y Verificación ("Doctor")

Para verificar el estado completo del sistema en cualquier momento:

```powershell
.\scripts\run_production_doctor.ps1
```

O desde línea de comandos directa:

```cmd
adbhub-agent.exe doctor
```

Salida esperada de un sistema listo para producción:
```text
================================================================================
 ADBHUB-LINK Production Readiness & System Diagnostic (Doctor)
================================================================================
 [1] Operating System Environment: Microsoft Windows 11 / 10 x64
 [2] Driver Signature Enforcement: ACTIVE (TESTSIGNING OFF / Secure Boot compliant)
 [3] Host Component (usbipd-win) : INSTALLED & SIGNED
 [4] Client Component (usbip-win2): INSTALLED & SIGNED (UDE Virtual USB Host Controller)
 [5] Local File Persistence      : VERIFIED (./data)
 [6] Network Ports               : TCP 3240 (Available), TCP 3420 (Available)
 [7] Workspace Test Suite        : 40 test suites OK (100% Passed)
================================================================================
```

---

## 5. Simulación y Pruebas de Laboratorio de Protocolos Móviles

Antes de conectar dispositivos físicos para flasheo real:

```powershell
.\scripts\simulate_lab_tests.ps1
```

Verifica la secuencia completa de los 6 modos:
1. **ADB**: Handshake y transferencias bulk bidireccionales.
2. **MTP**: Descriptores de clase WPD `06/01/01`.
3. **Qualcomm EDL 9008**: Descriptores de volcado y flasheo Sahara/Firehose.
4. **MediaTek BROM**: Captura en ventana de 3 segundos de arranque y pre-reserva de puerto.
5. **Spreadtrum / Unisoc**: Identificación lógica persistente y modo Download U2S.
6. **Samsung Download Mode (Odin)**: Handshake `"ODIN"` $\rightarrow$ `"LOKE"`, tolerancia de paquetes cero (ZLP), soporte de borrado UFS de 60 segundos y guardia contra desconexión accidental durante flasheo activo.

