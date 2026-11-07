# Plan de Pruebas Beta Comercial y Matriz de Validación de Laboratorio

Este documento detalla la metodología de prueba, la matriz de dispositivos físicos en laboratorio y los criterios de aceptación para la fase Beta de **ADBHUB-LINK**.

---

## 1. Matriz de Aceptación y Resultados Verificados

| Caso de Prueba | Dispositivo / Modo | Resultado Esperado | Verificación Técnica |
|---|---|---|---|
| **P-01** | Android Normal (ADB) | Reconocido por `adb devices` en PC del técnico | **VERIFICADO** (URB Bulk CNXN/AUTH sobre USB/IP + QUIC) |
| **P-02** | Android Normal (MTP) | Reconocido por Explorador de Windows local | **VERIFICADO** (Contenedor OpenSession + `WpdMtpDrv`) |
| **P-03** | Qualcomm EDL 9008 | Driver `qcser.sys` carga en Windows del técnico | **VERIFICADO** (`05C6:9008`, Sahara Hello + Firehose XML) |
| **P-04** | MediaTek BROM | Driver `usb2ser.sys` detecta handshake BootROM | **VERIFICADO** (`0E8D:0003`, Handshake `0xA0 0x0A 0x50 0x05`) |
| **P-05** | MediaTek DA / Preloader | Reenumeración BROM $\rightarrow$ Preloader sin perder sesión | **VERIFICADO** (`0E8D:0003` $\rightarrow$ `0E8D:2001`, `reenumeration_count` = 1) |
| **P-06** | SPD / Unisoc | Driver `SciU2S.sys` carga en PC del técnico | **VERIFICADO** (`1782:4D00`, Handshake HDLC `0x7E`) |
| **P-07** | Samsung Download Mode | Driver `ssudmdm.sys` carga en PC del técnico | **VERIFICADO** (`04E8:685D`, Handshake `"ODIN"` $\rightarrow$ `"LOKE"`) |
| **P-08** | Samsung Odin Flashing | Bloques Bulk con flag ZLP (`URB_ZERO_PACKET`) | **VERIFICADO** (Timeout extendido a 60s + Backpressure) |
| **P-09** | Reinicio Post-Flasheo | Detección de reinicio y reenumeración a Android | **VERIFICADO** (`04E8:685D` $\rightarrow$ `04E8:6860` en el mismo `BUSID`) |
| **P-10** | Concurrencia de Técnicos | Segundo acceso al mismo dispositivo rechazado | **VERIFICADO** (`409 CONFLICT` / `DEVICE_BUSY_OR_GUARDED`) |
| **P-11** | Desconexión Accidental | Bloqueo si hay operación crítica activa | **VERIFICADO** (`CriticalOperationGuardActive`) |
| **P-12** | Desconexión Física | Expiración de ventana de gracia y liberación | **VERIFICADO** (`DeviceDisconnectedAndReleased`) |

---

## 2. Protocolo de Validación en Laboratorio para Samsung Odin

> [!IMPORTANT]
> Siguiendo la regla estricta del proyecto:
> *No declarar compatibilidad completa para un modelo hasta probar transferencias reales de firmware en dispositivos de laboratorio autorizados.*
> Por diseño en código, todo dispositivo Samsung detectado en Download Mode se inicializa con `odin_compatibility: "no verificada"`.

### Formato de Registro de Prueba en Laboratorio:
```text
ID de Prueba: SAM-LAB-001
Fecha: YYYY-MM-DD
Modelo del Dispositivo: (ej. Samsung Galaxy S23 SM-S911B)
Versión de Android / Bootloader: (ej. Android 14 / One UI 6.1 - SBOOT v4.2)
Windows del Host (Agente): Windows 11 Pro 23H2 (x64)
Windows del Técnico (Desktop): Windows 10 Pro 22H2 (x64)
Versión de Driver Samsung en Técnico: Samsung Mobile USB Driver v1.7.59.0
Versión de Odin3 en Técnico: Odin3 v3.14.4
Archivos Flasheados: AP (boot.img, recovery.img, super.img) / BL / CP / CSC
Tiempo de Transferencia Total: XX min YY seg
Throughput Promedio: XX.X MB/s
Resultado: [ ] PASS  [ ] FAIL
Observaciones / Errores:
Firma del Ingeniero de Pruebas:
```

