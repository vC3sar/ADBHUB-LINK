# Especificación de API REST y Señalización WebSocket (Control Plane)

## 1. Autenticación

Todas las rutas protegidas requieren el encabezado HTTP:
```http
Authorization: Bearer <access_token>
```

- `POST /api/v1/auth/register` — Registra usuario y devuelve `TokenPairResponse` (`access_token`, `refresh_token`, `user`).
- `POST /api/v1/auth/login` — Autentica credenciales y devuelve `TokenPairResponse`.
- `POST /api/v1/auth/refresh` — Rota tokens usando un `refresh_token` válido.

---

## 2. Agentes y Dispositivos

- `POST /api/v1/agents/register` — Registra un nuevo `ADBHUB-LINK Agent` asociado al propietario autenticado.
- `POST /api/v1/agents/heartbeat` — Actualiza el estado en línea del agente y sincroniza la lista de dispositivos USB conectados (preservando el UUID lógico del dispositivo durante reenumeraciones en el mismo `busid`).
- `GET /api/v1/agents` — Lista todos los agentes registrados.
- `GET /api/v1/agents/{id}/devices` — Lista los dispositivos USB de un agente específico.
- `GET /api/v1/devices` — Lista todos los dispositivos USB con su modo clasificado (`AndroidAdb`, `Mtp`, `QualcommEdl`, `MediatekBrom`, `SpdUnisoc`, `SamsungDownload`) y estado `odin_compatibility`.
- `GET /api/v1/devices/{id}` — Obtiene el detalle de un dispositivo específico.
- `POST /api/v1/devices/{id}/share` — Autoriza el uso remoto del dispositivo (`AVAILABLE -> SHARED`).
- `POST /api/v1/devices/{id}/unshare` — Revoca la compartición del dispositivo (rechaza con `409 CONFLICT` si hay una sesión activa).

---

## 3. Sesiones Exclusivas y Emisión de Tokens QUIC Relay

- `POST /api/v1/sessions` — Solicita acceso exclusivo a un dispositivo compartido. Si otro técnico ya tiene reservado o activo el dispositivo, responde `409 CONFLICT` (`DEVICE_BUSY_OR_GUARDED`).
- `GET /api/v1/sessions/{id}` — Consulta el estado de la sesión.
- `POST /api/v1/sessions/{id}/approve` — El propietario aprueba la sesión (`PENDING_APPROVAL -> APPROVED`).
- `POST /api/v1/sessions/{id}/connect` — Activa la sesión (`APPROVED -> ACTIVE`) y emite el `SessionConnectBundle` con dos tokens criptográficos `SignedRelayToken` (`agent_relay_token` y `technician_relay_token`) para emparejar los flujos QUIC en el Relay.
- `POST /api/v1/sessions/{id}/disconnect` — Cierra o revoca inmediatamente la sesión y libera el bloqueo exclusivo del dispositivo.

---

## 4. Auditoría, Salud y Uso

- `GET /api/v1/audit` — Devuelve los últimos eventos de auditoría (sin almacenar jamás payloads USB).
- `GET /api/v1/health` — Estado de salud del Control Plane.
- `GET /api/v1/usage` — Métricas en tiempo real de usuarios, agentes, dispositivos, sesiones activas y eventos de auditoría.

---

## 5. Canales de Señalización WebSocket

- `GET /ws/agent` — Canal de eventos en tiempo real para el servicio Windows Agent.
- `GET /ws/desktop` — Canal de eventos en tiempo real para la aplicación Windows Desktop del técnico.
- `GET /ws/session/{id}` — Canal filtrado exclusivamente para los eventos de una sesión activa.

