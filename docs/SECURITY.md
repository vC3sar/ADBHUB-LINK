# Política de Seguridad, Consentimiento y Aislamiento de Procesos

## 1. Principios de Seguridad Fundamentales

1. **Consentimiento Explícito del Host**:
   - Ningún dispositivo USB conectado físicamente a la PC anfitriona puede ser exportado o conectado remotamente sin que el operador del anfitrión lo autorice explícitamente mediante el comando de compartición (`POST /api/v1/devices/{id}/share`).
2. **Exclusividad por Dispositivo**:
   - Cada dispositivo USB solo puede pertenecer a una sesión activa en un momento dado.
   - Si un segundo técnico intenta conectarse, la solicitud es rechazada inmediatamente con `409 CONFLICT` (`AdbHubError::DeviceBusy`).
3. **Revocación Inmediata**:
   - El operador del equipo anfitrión puede revocar la sesión en cualquier momento.
   - El Agente y el Relay invalidan los tokens inmediatamente y desconectan el flujo de datos.
4. **Protección de Dispositivos contra Bloqueo Accidental (Brick Prevention)**:
   - Durante operaciones de flasheo prolongadas (Samsung Download Mode, Qualcomm EDL, MediaTek BROM), el sistema activa automáticamente la bandera `critical_flashing_guard`.
   - Se bloquea la desconexión accidental desde la UI a menos que el usuario confirme explícitamente (`force = true`).
5. **Privacidad Estricta de Datos**:
   - **No se almacenan payloads USB** ni contenidos transferidos por los teléfonos en bases de datos ni en disco.
   - Los números de serie físicos se transforman en hashes unidireccionales SHA-256 (`ADBHUB-LINK-SERIAL-V1:<serial>`) antes de transmitirse al Control Plane.
   - No se almacenan claves privadas ADB ni credenciales propietarias.
6. **Cumplimiento Legal y Ético**:
   - `ADBHUB-LINK` es una solución legítima y profesional de transporte USB passthrough autorizado.
   - **No implementa ni desarrollará mecanismos para evadir FRP, Knox Guard (KG), MDM, Secure Boot ni protecciones de fábrica**.

