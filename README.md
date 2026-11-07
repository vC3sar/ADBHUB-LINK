# ADBHUB-LINK

**Plataforma profesional de USB Passthrough remoto multiprotocolo para reparación y flasheo de dispositivos móviles en Windows.**

`ADBHUB-LINK` transporta dispositivos USB físicos a nivel de paquetes URB (mediante el protocolo estándar USB/IP encapsulado sobre túneles QUIC autenticados con TLS 1.3) desde una computadora Windows anfitriona (donde está conectado físicamente el teléfono) hasta la computadora Windows del técnico a través de Internet.

Esto permite que el sistema operativo Windows del técnico enumere el dispositivo en su propio stack USB virtual (`vadimgrn/usbip-win2` UDE) y utilice sus controladores locales y herramientas profesionales (como **Samsung Odin3, Qualcomm QFIL, SP Flash Tool, ResearchDownload y Android Debug Bridge**) exactamente como si el teléfono estuviera conectado localmente a un puerto USB físico, sin emulación de alto nivel ni ejecución remota en el host.

---

## 1. Modos USB Multiprotocolo Soportados

1. **Android Debug Bridge (ADB)** (`0xFF / 0x42 / 0x01`): Comandos y depuración directa con `adb devices` local.
2. **Media Transfer Protocol (MTP)** (`0x06 / 0x01 / 0x01`): Integración con el subsistema WPD de Windows y explorador de archivos.
3. **Qualcomm EDL 9008** (`05C6:9008`): Soporte para Sahara / Firehose en herramientas como QFIL.
4. **MediaTek BROM & Preloader** (`0E8D:0003`, `0E8D:2000`, `0E8D:2001`): Captura durante la ventana de arranque de 3 segundos y pre-reserva de puerto.
5. **SPD / Spreadtrum / Unisoc** (`1782:4D00`, `1782:4D03`): Persistencia de identidad lógica en modos Download y Diag U2S.
6. **Samsung Download Mode (Odin / Loke)** (`04E8:685D`, `04E8:6601`, `04E8:68C3`):
   - Apretón de manos `"ODIN"` $\rightarrow$ `"LOKE"`.
   - Preservación estricta de paquetes de longitud cero (`USBIP_URB_ZERO_PACKET`).
   - Ventana de timeout de URB configurada a 60 segundos para borrado UFS de particiones grandes.
   - Panel específico de diagnóstico con guardia contra desconexión accidental durante flasheo activo.
   - Correlación automática de reenumeración post-flasheo (Download Mode $\rightarrow$ Android ADB).

---

## 2. Arquitectura del Sistema

```text
       CUSTOMER WINDOWS PC (HOST)
                │
     [Físico] Teléfono Móvil
                │
                ▼
       ADBHUB-LINK Agent
         (Rust / Tokio)
                │
       dorssel/usbipd-win
    (WHQL Signed Filter Driver)
                │
              USB/IP
                │
                ▼
       TLS 1.3 / QUIC Relay ◄─── Control Plane (REST / WebSockets)
         (Port UDP:4433)         (Local File Persistence ./data/)
                │
                ▼
       ADBHUB-LINK Desktop
       (Tauri 2 / Web UI)
                │
       vadimgrn/usbip-win2
     (Signed UDE KMDF Driver)
                │
   Windows Local USB OEM Drivers
 (ssudmdm.sys, qcser.sys, etc.)
                │
   ┌────────────┴────────────┐
  ADB / MTP                Odin3 / QFIL
 (Técnico Local)          (Flasheo Remoto)
```

### Regla Fundamental: Cumplimiento de Seguridad
- **Cero desactivación de firmas**: `TESTSIGNING ON` está estrictamente prohibido. Ambos extremos operan con drivers de kernel debidamente firmados (WHQL/Attestation y Open Source Codesigning Initiative).
- **Cero Docker y Cero Servidores DB Externos**: El Control Plane almacena su inventario, sesiones y auditoría en archivos JSON/JSONL transaccionales en `./data/`.

---

## 3. Estructura del Workspace

```text
ADBHUB-LINK/
├── apps/
│   ├── adbhub-agent/              # CLI / Servicio Windows del Agente Host
│   └── adbhub-desktop/            # Backend de la Aplicación de Escritorio del Técnico
├── crates/
│   ├── core/                      # Tipos de dominio, parsing binario USB y clasificador
│   ├── usb-enumerator/            # Win32 SetupAPI nativo y parser de usbipd-win
│   ├── usb-discovery/             # Identidad topológica USB v5 y filtro de reparación móvil
│   ├── usb-monitor/               # Monitor hotplug y máquina de estados con guardia de flasheo
│   ├── usbip-adapter/             # Códec binario USB/IP v1.1.1 y controlador de exportación
│   ├── usb-exporter/              # Gestor de URBs con timeouts por modo y backpressure (32 MiB)
│   ├── usbip-client-adapter/      # Adaptador de cliente usbip-win2 y puente de loopback VHCI
│   ├── relay-protocol/            # Tokens HMAC-SHA256, no-repudio y tramas de control
│   ├── secure-transport/          # Transporte QUIC (Quinn + Rustls TLS 1.3)
│   ├── relay-client/              # Cliente QUIC bidireccional saliente y puente de datos
│   ├── auth/                      # Autenticación, contraseñas hash y persistencia en users.json
│   ├── audit/                     # Registro de eventos de auditoría en audit_events.jsonl
│   ├── device-registry/           # Inventario de agentes y dispositivos en registry.json
│   ├── device-broker/             # Exclusividad estricta y bloqueo de sesiones concurrentes
│   └── session-manager/           # Ciclo de vida de sesiones y persistencia en sessions.json
├── services/
│   ├── api/                       # API REST Axum (19 endpoints) y canales WebSockets
│   └── relay/                     # Servidor de Relay QUIC emparejador de flujos
├── frontend/
│   └── desktop/                   # Interfaz de usuario para técnicos (HTML5/TypeScript)
├── installer/
│   ├── agent/                     # Scripts de instalación/desinstalación del Agente Host
│   ├── desktop/                   # Scripts de instalación/desinstalación de Desktop
│   └── InnoSetup/                 # Instalador unificado Inno Setup 6 (ADBHUB-LINK-Setup.exe)
├── scripts/
│   ├── run_production_doctor.ps1  # Diagnóstico completo de sistema y controladores
│   └── simulate_lab_tests.ps1     # Simulador de pruebas de laboratorio de los 6 modos
└── docs/                          # Documentación técnica, arquitectónica y guías de laboratorio
```

---

## 4. Instalación y Puesta en Marcha

### Prerrequisitos
- **Windows 10 / Windows 11 (x64)** con Secure Boot activado.
- **Rust 1.80+** (para compilación desde código fuente).

### Instalación Rápida con PowerShell (Administrador)

```powershell
# En la PC del Cliente / Taller (Host):
Set-ExecutionPolicy Bypass -Scope Process
.\installer\agent\install-agent.ps1

# En la PC del Técnico (Technician):
Set-ExecutionPolicy Bypass -Scope Process
.\installer\desktop\install-desktop.ps1
```

### Diagnóstico del Sistema ("Doctor")

```powershell
.\scripts\run_production_doctor.ps1
```

### Ejecución de Pruebas de Laboratorio

```powershell
.\scripts\simulate_lab_tests.ps1
```

---

## 5. Documentación de Referencia

- [Arquitectura Detallada](docs/ARCHITECTURE.md)
- [Auditoría de Componentes USB/IP Windows](docs/USBIP.md)
- [Matriz de Compatibilidad Multiprotocolo](docs/USB_COMPATIBILITY.md)
- [Protocolo y Flasheo en Samsung Download Mode](docs/SAMSUNG_DOWNLOAD_MODE.md)
- [Requisitos de Controladores y Firmas Digitales](docs/WINDOWS_DRIVERS.md)
- [Especificación del Protocolo QUIC Relay](docs/RELAY_PROTOCOL.md)
- [Documentación de la API REST y WebSockets](docs/API.md)
- [Modelo de Seguridad y Auditoría](docs/SECURITY.md)
- [Guía de Instalación y Prerrequisitos](docs/INSTALLATION.md)
- [Guía de Despliegue en Producción](docs/PRODUCTION_DEPLOYMENT.md)
- [Plan de Pruebas de Laboratorio Beta con Dispositivos Reales](docs/BETA_TEST_PLAN.md)
