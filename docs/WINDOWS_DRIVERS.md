# Controladores de Windows y Requisitos de Firma Digital

## 1. Política de Seguridad de Controladores en ADBHUB-LINK

Siguiendo las reglas estrictas de ingeniería del proyecto:
- **Jamás** se distribuyen controladores de kernel sin firma digital válida.
- **Jamás** se solicita al usuario ni al técnico desactivar *Driver Signature Enforcement* (`bcdedit /set testsigning on`) ni *Secure Boot*.
- **Jamás** se instalan certificados raíz autofirmados en el almacén de confianza del sistema.

---

## 2. Controladores en el Equipo Anfitrión (Cliente — `ADBHUB-LINK Agent`)

En el equipo donde se conecta físicamente el teléfono móvil:
- **Componente requerido**: `usbipd-win` (v4.0.0 o superior).
- **Drivers involucrados**:
  - `VBoxUsbMon.sys`: Monitor de filtro USB firmado para interceptar solicitudes de vinculación por `BUSID`.
  - `VBoxUsb.sys`: Controlador stub firmado que toma posesión exclusiva del dispositivo USB exportado para redirigir sus URBs hacia el servicio `usbipd`.
- **Nota operativa**: El equipo anfitrión **no necesita** tener instalados los drivers de Samsung, MediaTek, Spreadtrum ni Qualcomm 9008 para que el técnico trabaje, ya que cuando `usbipd-win` vincula (`bind`) el dispositivo por su `BUSID`, Windows sustituye el driver funcional del host por el driver stub de exportación USB/IP.

---

## 3. Controladores en el Equipo del Técnico (`ADBHUB-LINK Desktop`)

En el equipo desde donde se opera remotamente el teléfono:

### 3.1 Controlador de Bus Virtual USB/IP
- **Componente requerido**: `vadimgrn/usbip-win2`.
- **Drivers involucrados**:
  - `usbip2_ude.sys`: Controlador de emulación de dispositivo USB basado en el framework oficial de Microsoft **UDE (USB Device Emulation - `Udecx`)**.
  - `usbip2_filter.sys`: Filtro de soporte para operaciones de control y estado.
- **Firma**: Firmado para Windows 10 (x64, versión 1903+) y Windows 11 (x64) mediante la *Open Source Codesigning Initiative* / WHLK.

### 3.2 Controladores OEM para Herramientas de Reparación (Instalados en PC del Técnico)
Dado que el dispositivo aparece en la PC del técnico como un dispositivo USB real conectado al bus UDE, el técnico utiliza sus drivers habituales:
1. **ADB / Fastboot**: Google USB Driver (`winusb.sys` / `android_winusb.inf`).
2. **MTP**: Controlador nativo de Windows (`wpdmtp.inf` / `WinUsb.sys`).
3. **Qualcomm EDL 9008**: Qualcomm USB Driver v1.0+ (`qcser.sys` / `qcusbser.sys`).
4. **MediaTek BROM / Preloader**: MediaTek SP Driver (`usb2ser.sys`).
5. **SPD / Unisoc**: SCI USB2Serial Driver (`SciU2S.sys`).
6. **Samsung Download Mode (Odin)**: Samsung Android USB Driver for Windows v1.7+ (`ssudbus.sys`, `ssudmdm.sys`).

