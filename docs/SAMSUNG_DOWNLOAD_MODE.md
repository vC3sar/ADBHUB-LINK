# Samsung Download Mode y Compatibilidad con Odin sobre USB/IP

> [!WARNING]
> **ADBHUB-LINK** es exclusivamente un sistema de transporte USB passthrough autorizado. No interpreta ni altera paquetes `.tar.md5`, no emula Odin en el host, y jamás implementa mecanismos para evadir FRP, Knox Guard (KG), MDM o Secure Boot.

---

## 1. Arquitectura de Transporte para Samsung Download Mode

Cuando un teléfono Samsung entra en **Download Mode** (también conocido internamente por su bootloader secundario `SBOOT` / protocolo **Loke** / **Odin**), presenta la siguiente topología USB:

- **Vendor ID (`idVendor`)**: `0x04E8` (Samsung Electronics Co., Ltd.)
- **Product ID (`idProduct`)**:
  - `0x685D`: Dispositivos Galaxy modernos (Series S, Note, Z, A, M, Tab) en modo Download.
  - `0x6601` / `0x68C3`: Variantes de Download Mode en plataformas específicas o legadas.
- **Configuración e Interfaces**:
  - **Interfaz 0 — Communications Control (`bInterfaceClass = 0x02`, `bInterfaceSubClass = 0x02` ACM)**:
    - Endpoint `0x81` o `0x82` (Interrupt IN): Notificaciones de estado de línea serial.
  - **Interfaz 1 — CDC Data (`bInterfaceClass = 0x0A`, `bInterfaceSubClass = 0x00`)**:
    - Endpoint Bulk IN (ej. `0x81` / `0x82`, `wMaxPacketSize = 512` bytes en High-Speed).
    - Endpoint Bulk OUT (ej. `0x01` / `0x02`, `wMaxPacketSize = 512` bytes en High-Speed).

---

## 2. Flujo de Reconocimiento Local en el Windows del Técnico

1. El teléfono entra en Download Mode en la PC anfitriona (ya sea conectado directamente con Vol+ y Vol- o tras un reinicio desde Android `04E8:6860` mediante `adb reboot download`).
2. `adbhub-agent` detecta el cambio de modo (`AndroidAdb` $\rightarrow$ `SamsungDownload`), mantiene la asociación lógica de la sesión gracias a la topología del puerto (`BUSID`), y actualiza la exportación USB/IP.
3. En la PC del técnico, el controlador virtual `usbip-win2` (UDE) enumera el dispositivo `USB\VID_04E8&PID_685D`.
4. El Plug and Play de Windows del técnico carga los controladores oficiales **Samsung Android USB Driver for Windows** instalados localmente:
   - `ssudbus.sys` (SAMSUNG Mobile USB Composite Device Driver)
   - `ssudmdm.sys` (SAMSUNG Mobile USB Modem — crea el puerto `COMx` que Odin enumera bajo `ID:COM` como `0:[COMx]` en color azul).
5. **Odin3** ejecutándose en la PC del técnico detecta el puerto y realiza el handshake inicial:
   - Envía `"ODIN"` (4 bytes ASCII) por Bulk OUT.
   - El teléfono responde `"LOKE"` (4 bytes ASCII) por Bulk IN.

---

## 3. Consideraciones Críticas del Protocolo Odin sobre Redes WAN (QUIC Relay)

El flasheo de firmware Samsung (`BL`, `AP`, `CP`, `CSC` / `HOME_CSC`) involucra particularidades a nivel URB que pueden fallar si un túnel USB/IP genérico no está preparado:

### 3.1 Inicialización, Lectura de PIT y Timeouts de Borrado
- Al iniciar la sesión (`SetupConnection`), Odin solicita la tabla de particiones (**PIT — Partition Information Table**) en bloques de 500 bytes y luego envía cabeceras de sesión de 1024 bytes (`0x64` = Begin Session, `0x65` = Pit File, `0x66` = File Transfer, `0x67` = End Session).
- Antes de escribir una partición grande (como `super.img` de 6 GB a 12 GB), el bootloader del teléfono borra o prepara bloques UFS/eMMC. Durante este tiempo, la respuesta al paquete de control/bulk puede tardar entre **5 y 25 segundos**.
- **Regla en ADBHUB-LINK**: Para cualquier sesión identificada en modo `SamsungDownload`, el timeout de URBs pendientes en el Relay y en el adaptador USB/IP se eleva a **60,000 ms (60 segundos)** por operación, evitando desconexiones falsas mientras el chip UFS del teléfono procesa comandos de borrado.

### 3.2 Transferencias Bulk de Gran Volumen y Zero-Length Packets (ZLP)
- Odin transmite los archivos de imagen en secuencias (`Sequence`) de por ejemplo 30 MB (`0x01E00000` bytes), divididas en bloques USB Bulk OUT de hasta 1 MB (`1,048,576` bytes) o múltiplos de `wMaxPacketSize` (512 bytes).
- Cuando una transferencia es múltiplo exacto de 512 bytes y la bandera `USBD_SHORT_TRANSFER_OK` / `URB_ZERO_PACKET` está presente, el stack USB debe preservar fielmente dicho flag en el header `USBIP_CMD_SUBMIT` (`transfer_flags`).

### 3.3 Control de Flujo (Backpressure) y Límites de Memoria
- Odin en una PC rápida puede encolar decenas de megabytes por segundo hacia el driver local. Si el enlace de subida del técnico o de bajada del cliente es de p. ej. 50 Mbps (~6 MB/s), un relay sin backpressure acumularía cientos de megabytes en RAM y provocaría que Odin crea que los bloques ya llegaron al teléfono, causando un timeout al final de la secuencia de 30 MB cuando espera el ACK de 8 bytes del bootloader.
- **Solución**: El adaptador de cliente y el Relay QUIC aplican **backpressure estricto de ventana de URB**: no se confirma el `USBIP_RET_SUBMIT` al driver UDE del técnico hasta que el Agente del anfitrión recibe el completado real del hardware USB físico (`IoCompletion` en `VBoxUsb`).

### 3.4 Protección contra Desconexión Accidental y Estado de Verificación
- La UI y la máquina de estados del Agente marcan la sesión con la bandera `critical_operation_guard` cuando el dispositivo está en `SamsungDownload` con tráfico Bulk OUT sostenido.
- **Estado de Compatibilidad**: En el código (`OdinCompatibilityStatus`), el estado por defecto de todo modelo Samsung detectado es `Unverified` (No verificado en laboratorio) hasta que el modelo específico esté registrado en la matriz de dispositivos validados con pruebas reales de flasheo autorizado.

