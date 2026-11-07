# Auditoría Técnica de Componentes USB/IP para Windows

> [!IMPORTANT]
> Este documento constituye el entregable de la **Primera Tarea (Sección 17)** y de la **Sección 5 (Componentes USB Existentes)**. Documenta la comparación técnica, licencias, estado de firma WHQL/Attestation, arquitectura de drivers y limitaciones verificadas antes de escribir el subsistema de exportación y cliente.

---

## 1. Comparativa de Alternativas USB/IP en Windows

Para lograr **USB Passthrough real** en Windows 10 y Windows 11 (x64) sin obligar a los usuarios a desactivar *Driver Signature Enforcement* (`bcdedit /set testsigning on`), se auditaron las cuatro alternativas principales del ecosistema:

| Criterio | `dorssel/usbipd-win` (Servidor) | `vadimgrn/usbip-win2` (Cliente / Servidor) | `cezanne/usbip-win` (Legacy) | SDKs Comerciales (VirtualHere / FabulaTech / Eltima) |
|---|---|---|---|---|
| **Rol Principal** | **Servidor (Host Export)** en Windows | **Cliente (VHCI/UDE)** y Servidor en Windows | Cliente y Servidor en Windows | Cliente y Servidor propietarios |
| **Estado de Mantenimiento** | **Activo (Estándar oficial promovido por Microsoft para WSL2)** | **Activo (Reescritura moderna C++20 / WSK)** | **Inactivo / Abandonado** (últimos releases estables requieren parches manuales) | Activo (Código cerrado) |
| **Licencia** | **GPL-3.0-only** | **BSD-2-Clause** (User-mode & Drivers) / GPLv2 en utilidades específicas | GPL-2.0 | Propietaria (Costo por seat/servidor muy elevado: \$1,500–\$10,000+ USD OEM) |
| **Arquitectura de Driver** | `VBoxUsb` (Driver de reemplazo / filtro de dispositivo bajo control de `usbipd`) | `usbip2_ude` (USB Device Emulation KMDF) + Winsock Kernel (WSK) | `usbip_vhci` (WDM / UDE experimental) | Drivers filtro y bus virtual propietarios |
| **Firma Digital (Win 10/11 x64)** | **Firmado por Microsoft / WHQL** (Instalador `.msi` oficial no requiere Test Mode) | **Firmado (Attestation / WHLK via Open Source Codesigning Initiative)** — No requiere Test Mode | **No firmado comercialmente** (Requiere certificado de prueba y `TESTSIGNING ON`) | Firmado WHQL |
| **Soporte Dispositivos Compuestos** | Sí (captura a nivel de dispositivo completo por `BUSID`) | Sí (emula el dispositivo completo sobre el bus virtual UDE) | Parcial / Inestable con interfaces múltiples complejas | Sí |

---

## 2. Selección Arquitectónica Definitiva

### 2.1 Servidor (Lado PC Anfitrión — `ADBHUB-LINK Agent`)
**Selección: `dorssel/usbipd-win` (v4.x+)**
- **Justificación técnica**:
  1. Es la solución de referencia respaldada por Microsoft para exportar dispositivos USB desde un host Windows.
  2. Sus controladores (`VBoxUsb.sys` / `VBoxUsbMon.sys` empaquetados y firmados oficialmente) se instalan limpiamente en Windows 10 y Windows 11 x64 con Secure Boot activado.
  3. Opera a nivel de dispositivo completo identificado por `BUSID` (`<bus>-<port>`), lo que permite capturar tanto dispositivos de interfaz única (Qualcomm 9008, MTK BROM, Samsung Download Mode) como dispositivos compuestos complejos (Android con ADB + MTP + CDC ACM).
  4. Expone el protocolo estándar USB/IP (`127.0.0.1:3240` o mediante `usbipd state` / sockets locales) que nuestro `adbhub-agent` puede interceptar y tunelizar hacia el Relay QUIC sin exponer el puerto 3240 a la red local.

### 2.2 Cliente (Lado PC del Técnico — `ADBHUB-LINK Desktop`)
**Selección: `vadimgrn/usbip-win2` (Driver VHCI basado en UDE - USB Device Emulation)**
- **Justificación técnica**:
  1. A diferencia de `cezanne/usbip-win` (que exige desactivar la protección de firmas de Windows, violando la Regla 17 y 18), `vadimgrn/usbip-win2` cuenta con drivers firmados a través de la *Open Source Codesigning Initiative* compatibles con Windows 10 (1903+) y Windows 11 x64.
  2. Utiliza **Microsoft UDE (`Udecx`)** sobre KMDF, lo que significa que el dispositivo importado se presenta ante el Plug and Play (PnP) Manager de Windows como un dispositivo USB real conectado a un Host Controller virtual.
  3. Gracias a UDE, Windows carga de forma transparente los drivers locales instalados en la PC del técnico (`ssudbus.sys` / `ssudmdm.sys` de Samsung, `qcser.sys` de Qualcomm, `winusb.sys` para ADB/Fastboot, `wpdmtp.dll` para MTP).
  4. Su licencia **BSD-2-Clause** es óptima para integración comercial.

---

## 3. Análisis Legal y de Licencias para Distribución Comercial

1. **`dorssel/usbipd-win` (GPL-3.0)**:
   - **Impacto**: La licencia GPLv3 aplica al binario y código fuente de `usbipd-win`.
   - **Estrategia de Cumplimiento en `ADBHUB-LINK`**:
     - `ADBHUB-LINK Agent` **no enlaza estáticamente ni dinámicamente** código de `usbipd-win`, ni modifica sus binarios.
     - `usbipd-win` se instala como un componente del sistema independiente mediante su instalador oficial firmado `.msi` (o `winget`), y `adbhub-agent` interactúa con él mediante invocación de procesos CLI (`usbipd state`, `usbipd bind`, `usbipd unbind`) y comunicación de red por socket TCP estándar RFC/Linux USB/IP (`127.0.0.1:3240`).
     - En filosofía FSF/GPL, la comunicación por sockets estándar de protocolo (USB/IP) e invocación de línea de comandos con un programa separado constituye *mere aggregation* (agregación simple), manteniendo el código propietario de `ADBHUB-LINK` independiente, siempre que se incluya atribución clara y copia de la licencia GPLv3 para el instalador de `usbipd-win`.
2. **`vadimgrn/usbip-win2` (BSD-2-Clause)**:
   - **Impacto**: Permite uso comercial, redistribución binaria e integración con atribución de copyright en la documentación y ventana "Acerca de".
3. **Firma de Controladores (Reglas 17 y 18)**:
   - **Prohibición estricta**: `ADBHUB-LINK` jamás ejecutará `bcdedit /set testsigning on` ni instalará certificados raíz autofirmados en el almacén `ROOT` de la máquina del cliente o del técnico.
   - El instalador verificará la presencia de los controladores firmados oficiales de `usbipd-win` (en el Agente) y `usbip-win2` (en el Desktop).

---

## 4. Limitaciones Verificadas en Windows USB/IP

Durante la auditoría de la pila USB/IP en Windows se verificaron las siguientes limitaciones técnicas reales que el diseño de `ADBHUB-LINK` debe mitigar:

1. **Pérdida del `BUSID` o desvinculación durante la Reenumeración (Reboot / Cambio de Modo)**:
   - Cuando un teléfono cambia de modo (por ejemplo, de Android ADB `04E8:6860` a Samsung Download Mode `04E8:685D`, o de apagado a MediaTek BROM `0E8D:0003`), el dispositivo se desconecta eléctricamente del bus USB y vuelve a enumerarse con un nuevo `VID:PID` y nuevos descriptores.
   - Aunque el `BUSID` físico (`<bus>-<port>`, ej. `1-4`) suele mantenerse constante si está conectado al mismo puerto físico de la placa madre/hub, la sesión TCP de USB/IP activa se cierra abruptamente con un error de desconexión (`EPIPE` / `ECONNRESET`).
   - **Mitigación en ADBHUB-LINK**: Implementación del **`DeviceLifecycleManager`** en `crates/usb-monitor` que correlaciona el puerto topológico (`BUSID` / cadena de ubicación PnP) dentro de una ventana temporal de reenumeración, ejecuta automáticamente `usbipd bind --busid <busid>` sobre la nueva identidad si la sesión autoriza transiciones de modo, y reconecta el stream del técnico sin requerir una nueva aprobación manual.

2. **Ventana crítica de captura en MediaTek BROM y SPD BootROM (< 2.5 segundos)**:
   - Muchos SoCs MediaTek y Unisoc en modo BootROM esperan el handshake de la herramienta (ej. `0xA0 0x0A 0x50 0x05` en MTK BROM) durante apenas 1.5 a 3 segundos tras conectarse antes de continuar el arranque normal o apagarse por watchdog.
   - Si el técnico tuviera que hacer clic en "Conectar" manualmente después de que aparece el dispositivo BROM, el teléfono ya habría salido de BROM.
   - **Mitigación en ADBHUB-LINK**: Modo **"Auto-Bind por Puerto reservado (Pre-armed Port Session)"**. Cuando una sesión está activa o esperando reenumeración en un `BUSID` específico, el Agente enlaza y adjunta instantáneamente cualquier dispositivo que aparezca en ese puerto físico en milisegundos.

3. **Bloqueo de MTP por el servicio WPD de Windows Anfitrión**:
   - Cuando se conecta un teléfono en modo MTP al PC anfitrión, Windows carga inmediatamente `WinUsb`/`WpdMtpDrv` y el Explorador de Windows del anfitrión abre una sesión MTP.
   - Al ejecutar `usbipd bind`, `usbipd-win` fuerza una reenumeración suave o requiere `--force` para sustituir el driver funcional del host por el driver stub (`VBoxUsb`), liberando el bloqueo de WPD del host para que el Windows del técnico sea el único que abra la sesión MTP.

4. **Transferencias Bulk de Gran Tamaño y ZLP (Zero-Length Packets) en Samsung Odin**:
   - El protocolo Loke/Odin de Samsung envía paquetes de firmware (`system.img.ext4`, `super.img`, `vendor.img`) divididos en bloques grandes mediante transferencias USB Bulk OUT, seguidos de consultas de estado.
   - Sobre WAN con latencia (ej. 40–120 ms RTT), si las lecturas/escrituras URB son pequeñas y síncronas, el throughput cae drásticamente, o si un envío de 32 MB tarda más que el timeout de la llamada `WinUSB` / `ssudbus`, Odin aborta con `FAIL! (Write)`.
   - **Mitigación en ADBHUB-LINK**: Uso de QUIC con ventana de flujo amplia, desactivación del algoritmo de Nagle (`TCP_NODELAY`) en los puentes locales TCP `127.0.0.1:3240`, soporte fiel de `URB_ZERO_PACKET`, y timeouts extendidos para sesiones en modo `SamsungDownload`.

