# Guía de Instalación y Despliegue en Windows 10 y Windows 11 (x64)

## 1. Requisitos del Sistema

- **Sistema Operativo**: Windows 10 (versión 1903 o superior) o Windows 11 (x64).
- **Seguridad**: Compatible 100% con **Secure Boot** activado y **Driver Signature Enforcement** activado.
- **Herramientas de Desarrollo**: Rust 1.80+ (para compilar desde código fuente).

---

## 2. Instalación de Componentes USB/IP Firmados

### En la PC Anfitriona (donde se conecta físicamente el teléfono):
Instalar el servicio oficial firmado de `usbipd-win`:
```powershell
winget install dorssel.usbipd-win
```
*No requiere reiniciar ni poner Windows en modo de prueba (`TESTSIGNING`).*

### En la PC del Técnico (donde se ejecutan las herramientas de reparación):
Instalar el driver cliente firmado UDE de `usbip-win2`:
```powershell
winget install vadimgrn.usbip-win2
```
*Utiliza controladores certificados por Microsoft / Open Source Codesigning Initiative.*

---

## 3. Verificación del Sistema (`Doctor`)

Ejecutar la herramienta de diagnóstico integrada:
```powershell
cargo run -p adbhub-agent -- doctor
```

---

## 4. Ejecución del Sistema en Modo Local (Zero Docker / Zero Servers)

El sistema opera completamente sin contenedores ni bases de datos externas:

1. **Iniciar el Agente Anfitrión (Monitoreo USB y Exportación)**:
   ```powershell
   cargo run -p adbhub-agent -- --watch
   ```

2. **Iniciar el Control Plane y la Aplicación Desktop para Técnicos**:
   ```powershell
   cargo run -p adbhub-desktop
   ```
   Abrirá la interfaz en `http://127.0.0.1:3420` persistiendo usuarios, dispositivos, sesiones y auditoría en `./data/`.

3. **(Opcional para WAN) Iniciar el Servidor Relay QUIC Centralizado**:
   ```powershell
   cargo run -p adbhub-relay-service
   ```

