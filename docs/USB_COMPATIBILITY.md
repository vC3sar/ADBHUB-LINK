# Matriz de Compatibilidad Multiprotocolo y Análisis de Protocolos USB

Este documento detalla el análisis técnico de los 6 modos obligatorios de **ADBHUB-LINK** (Puntos 6 a 11 de la Auditoría de la Tarea 1): cómo se enumeran a nivel USB, qué descriptores e interfaces presentan, cómo interactúan con `usbipd-win` y `usbip-win2`, y qué limitaciones deben gestionarse.

---

## 1. ADB (Android Debug Bridge)

- **Identificación USB**:
  - `bInterfaceClass = 0xFF` (Vendor Specific)
  - `bInterfaceSubClass = 0x42` (ADB)
  - `bInterfaceProtocol = 0x01`
  - Frecuentemente parte de un **Dispositivo Compuesto** (`bDeviceClass = 0x00` o `0xEF` Miscellaneous / IAD `0x02`/`0x01`), compartiendo configuración con MTP, PTP, RNDIS o diagnóstico.
- **VIDs Comunes**:
  - Google (`18D1`), Samsung (`04E8`), Xiaomi (`2717`), Motorola (`22B8`), Huawei (`12D1`), OnePlus/ Oppo (`22D9`), Vivo (`2D95`), Sony (`0FCE`), LG (`1004`).
- **Comportamiento USB/IP**:
  - Utiliza 2 endpoints **Bulk** (1 Bulk IN, 1 Bulk OUT).
  - Paquetes de transporte ADB (`CNXN`, `AUTH`, `OPEN`, `WRTE`, `OKAY`, `CLSE`) con cabeceras de 24 bytes seguidas de payload.
  - Tolerante a latencias moderadas (hasta ~250 ms RTT sin pérdida de sesión).
- **Consideraciones en Windows**:
  - El servidor `adb.exe` debe correr exclusivamente en la PC del técnico. Si la PC anfitriona tiene un `adb.exe` corriendo en segundo plano, cuando `usbipd-win` enlaza el dispositivo al driver `VBoxUsb`, el servidor `adb.exe` local del anfitrión pierde el handle y el acceso pasa íntegramente al técnico.

---

## 2. MTP (Media Transfer Protocol)

- **Identificación USB**:
  - Interfaz con `bInterfaceClass = 0x06` (Still Image Capture), `bInterfaceSubClass = 0x01`, `bInterfaceProtocol = 0x01`, **o**
  - `bInterfaceClass = 0xFF` (Vendor Specific) acompañado de descriptores Microsoft OS String (`0xEE`: `MSFT100`) con `compatibleID = "MTP"` o descriptor de interfaz con nombre `"MTP"`.
- **Endpoints**:
  - 1 Bulk IN (Datos desde el dispositivo).
  - 1 Bulk OUT (Comandos MTP y datos hacia el dispositivo).
  - 1 Interrupt IN (Eventos asíncronos MTP como `ObjectAdded`, `StoreAdded`, `DevicePropChanged`).
- **Limitaciones Verificadas de MTP sobre USB/IP**:
  1. **Contención con el servicio WPD del Host**: En Windows, al conectar un teléfono en modo "Transferencia de archivos", el driver local `WpdMtpDrv.dll` abre una sesión MTP automáticamente (`OpenSession`). MTP **solo permite una sesión activa a la vez**. Al hacer `bind` con `usbipd-win`, el dispositivo se desacopla del stack WPD del host y se reenumera bajo `VBoxUsb`, permitiendo que el Explorador de Windows del técnico ejecute su propio `OpenSession`.
  2. **Enumeración de directorios con miles de fotos (`DCIM/Camera`)**: El Explorador de Windows realiza cientos de llamadas secuenciales `GetObjectHandles` y `GetObjectPropList`. Con latencias WAN > 80 ms, abrir una carpeta con 3,000 fotos puede tardar varios segundos. No existe corrupción de datos porque los contenedores MTP viajan sobre transferencias Bulk verificadas por CRC/TCP/QUIC, pero la UI debe informar la latencia de sesión.

---

## 3. Qualcomm EDL (Emergency Download Mode — 9008)

- **Identificación USB**:
  - `VID = 0x05C6` (Qualcomm, Inc.)
  - `PID = 0x9008` (Qualcomm HS-USB QDLoader 9008) — *También variantes de diagnóstico como `0x900E` (Emergency Tsar/Ramdump) o `0x9091`*.
  - Clase de interfaz: `0xFF` (Vendor Specific) con 1 Bulk IN y 1 Bulk OUT.
- **Protocolos Superiores Soportados Transparentemente**:
  - **Sahara Protocol**: Handshake inicial y carga del programador (`prog_emmc_firehose_*.mbn` / `prog_ufs_firehose_*.elf`) hacia la SRAM del SoC.
  - **Firehose Protocol**: Comandos XML sobre USB Bulk para lectura/escritura de sectores UFS/eMMC.
- **Compatibilidad y Limitaciones**:
  - En el PC del técnico, el dispositivo emulado por `usbip-win2` carga el driver local `qcser.sys` (Qualcomm HS-USB QDLoader 9008 COM port) o el driver `WinUSB` si el técnico utiliza herramientas directas libusb/WinUSB.
  - **Atención al Timeout de Sahara**: Cuando el dispositivo entra en 9008, el estado de la máquina de estados Sahara es único por encendido. Si un proceso en el PC anfitrión lee el paquete `Sahara Hello` inicial antes de que el dispositivo sea exportado a USB/IP, la herramienta del técnico (QFIL / MiFlash) no recibirá el `Hello` y fallará con `Sahara Fail`. Por ello, el **PC anfitrión NO debe tener instalado o activo ningún software que abra el puerto COM 9008**, o bien el puerto USB debe estar pre-vinculado (`bound`) a `usbipd-win` antes de conectar el teléfono en EDL.

---

## 4. MediaTek BROM y Preloader

- **Identificación USB**:
  - `VID = 0x0E8D` (MediaTek Inc.)
  - **Modo BROM (BootROM)**: `PID = 0x0003` (`MTK USB Port`).
  - **Modo Preloader**: `PID = 0x2000` (`MediaTek PreLoader USB VCOM`), `0x2001`, `0x0006` (`DA USB VCOM`).
  - Clase de dispositivo/interfaz: `0x02` (Communications and CDC Control) + `0x0A` (CDC-Data).
- **Endpoints**:
  - 1 Interrupt IN (Notificaciones CDC Serial).
  - 1 Bulk IN + 1 Bulk OUT (Flujo de datos BROM y Download Agent - DA).
- **Compatibilidad y Limitaciones Críticas**:
  1. **Ventana de Handshake de ~2 segundos**: Al conectar el teléfono apagado (o presionando Vol+ / Vol-), el BootROM espera la secuencia `0xA0 0x0A 0x50 0x05` durante ~2 segundos.
  2. **Solución Arquitectónica**: `usbipd-win` persiste el `bind` por `BUSID` en el registro de Windows (`HKLM\SYSTEM\CurrentControlSet\Services\usbipd`). Cuando el técnico activa el modo **"Captura Rápida BROM / Pre-Bind"** en `ADBHUB-LINK`, el puerto físico queda interceptado por el filtro de `usbipd-win` y el stream QUIC permanece pre-establecido para adjuntar el dispositivo en < 150 ms desde la detección del hotplug.
  3. **Reenumeración BROM → DA High-Speed**: Algunas herramientas cargan el DA1 en BROM (`0E8D:0003`) y el DA2 fuerza una reenumeración USB a High-Speed (`0E8D:2001`). El `DeviceLifecycleManager` detecta este cambio como una transición `MediatekBrom -> MediatekPreloader` en el mismo `BUSID` y mantiene la sesión del técnico viva.

---

## 5. SPD / Spreadtrum / Unisoc

- **Identificación USB**:
  - `VID = 0x1782` (Spreadtrum Communications Inc. / Unisoc)
  - `PID = 0x4D00` (SPRD U2S Diag / BootROM Download Mode), `0x4D03`, `0x3D00`.
  - Clase de interfaz: `0xFF` (Vendor Specific, 2 endpoints Bulk) o `0x02`/`0x0A` (CDC ACM).
- **Comportamiento en Herramientas Locales (SPD ResearchDownload / UpgradeDownload / FactoryDownload)**:
  - Carga del `FDL1` (First Stage Bootloader) en SRAM y `FDL2` en DDR mediante tramas HDLC (`0x7E ... 0x7E`).
  - Al iniciar `FDL2`, ciertos SoCs Unisoc (T606, T612, T616, SC9863A) realizan un re-attach USB rápido manteniendo el mismo `BUSID`.
  - El driver local del técnico (`SciU2S.sys` — *SPRD U2S Diag*) crea el puerto COM virtual en el Administrador de Dispositivos del técnico y se comunica directamente con `ResearchDownload.exe`.

---

## 6. Samsung Download Mode y Odin

- **Identificación USB**:
  - `VID = 0x04E8` (Samsung Electronics Co., Ltd.)
  - `PID = 0x685D` (Gadget Serial / Download Mode moderno en Galaxy S/A/M/Fold/Flip).
  - Otros PIDs históricos/específicos de Download Mode: `0x6601`, `0x68C3`.
  - Clase: Dispositivo CDC ACM / Modem (`bDeviceClass = 0x02` o compuesto con interfaces `0x02/0x02` Abstract Control Model + `0x0A/0x00` CDC Data).
- **Verificación detallada**: Ver [SAMSUNG_DOWNLOAD_MODE.md](file:///c:/Users/vCesar/OneDrive/Escritorio/Proyectos/ADBHUB-LINK/docs/SAMSUNG_DOWNLOAD_MODE.md) para el análisis profundo del protocolo Loke/Odin, gestión de ZLP, timeouts de particiones `super.img` y protecciones contra desconexión accidental.

---

## 7. Tabla Resumen de Modos y Firmas USB

| Modo USB | VIDs Principales | PIDs Principales | Clase / Subclase / Protocolo | Reenumeración Típica | Sensibilidad a Latencia |
|---|---|---|---|---|---|
| **ADB** | `18D1`, `04E8`, `2717`, `22B8`, `12D1`, `22D9` | Varios (`4EE2`, `6860`, etc.) | `FF / 42 / 01` | Al activar/desactivar depuración USB o `adb reboot` | Baja (< 250 ms RTT) |
| **MTP** | Todos los OEM Android | Varios (`4EE1`, `6860`, etc.) | `06 / 01 / 01` o `FF` ("MTP") | Al cambiar modo de carga a Transferencia de Archivos | Media (< 150 ms RTT recomendado) |
| **Qualcomm EDL** | `05C6` | `9008`, `900E` | `FF / FF / FF` o `FF / 00 / 00` | `adb reboot edl` o Test Point → `05C6:9008` | Media (Crítico no perder `Sahara Hello`) |
| **MediaTek BROM** | `0E8D` | `0003`, `2000`, `2001` | `02 / 02 / 01` + `0A / 00 / 00` | Apagado → BROM (`0003`) → Preloader/DA (`2001`) | **Alta** (Handshake inicial < 2s; requiere Pre-Bind) |
| **SPD / Unisoc** | `1782` | `4D00`, `4D03` | `FF / 00 / 00` o CDC `02`/`0A` | Apagado + Vol- → `1782:4D00` (FDL1 → FDL2) | Media-Alta (Handshake inicial ~3s) |
| **Samsung Download** | `04E8` | `685D`, `6601`, `68C3` | `02 / 02 / 01` + `0A / 00 / 00` | `adb reboot download` o Vol+ & Vol- → `04E8:685D` | Media (Crítico estabilidad Bulk en archivos > 8 GB) |

