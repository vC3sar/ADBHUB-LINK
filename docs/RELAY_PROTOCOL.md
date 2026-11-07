# Especificación del Protocolo de Relay QUIC (`adbhub-relay-v1`)

## 1. Arquitectura de Conexión Saliente (Zero Inbound Ports)

Tanto **ADBHUB-LINK Agent** (en el equipo Windows donde está conectado el teléfono) como **ADBHUB-LINK Desktop** (en el equipo Windows del técnico) establecen conexiones salientes UDP sobre **QUIC + TLS 1.3** (`ALPN = "adbhub-relay-v1"`) hacia el servidor **ADBHUB-LINK Relay**.

```mermaid
sequenceDiagram
    participant HostUSB as usbipd-win (127.0.0.1:3240)
    participant Agent as ADBHUB-LINK Agent
    participant Relay as ADBHUB-LINK QUIC Relay
    participant Desktop as ADBHUB-LINK Desktop
    participant VHCI as usbip-win2 UDE Driver

    Agent->>Relay: Outbound QUIC + TLS 1.3 (Open BiStream)
    Agent->>Relay: BindDataStream(SignedRelayToken[AgentHost], gen=0, ExportedDevice[312B])
    Desktop->>Relay: Outbound QUIC + TLS 1.3 (Open BiStream)
    Desktop->>Relay: BindDataStream(SignedRelayToken[TechnicianDesktop], gen=0)
    Relay-->>Agent: DataStreamPaired(session_id, gen=0)
    Relay-->>Desktop: DataStreamPaired(session_id, gen=0, ExportedDevice[312B])
    Desktop->>VHCI: LocalVhciBridgeServer + usbip.exe attach
    VHCI->>Desktop: OP_REQ_IMPORT (busid)
    Desktop-->>VHCI: OP_REP_IMPORT (312B Device Descriptor)
    VHCI->>Desktop: USBIP_CMD_SUBMIT (48B Header + URB Payload)
    Desktop->>Relay: Raw URB Stream (QUIC Flow Control)
    Relay->>Agent: Raw URB Stream (Backpressure 32 MiB)
    Agent->>HostUSB: USBIP_CMD_SUBMIT
    HostUSB-->>Agent: USBIP_RET_SUBMIT
    Agent-->>Relay: Raw URB Completion
    Relay-->>Desktop: Raw URB Completion
    Desktop-->>VHCI: USBIP_RET_SUBMIT
```

---

## 2. Seguridad, Autenticación y Protección Anti-Replay

1. **Tokens Efímeros Firmados (`SignedRelayToken`)**:
   - Cada flujo de datos requiere un token firmado con `HMAC-SHA256` que contiene:
     - `session_id`: UUID de la sesión autorizada.
     - `role`: `AgentHost` o `TechnicianDesktop`.
     - `peer_id`: UUID del Agente o Técnico.
     - `bus_id`: Puerto topológico autorizado (ej. `1-4`).
     - `nonce`: UUID v4 único de 128 bits.
     - `issued_at_unix` y `expires_at_unix`: Ventana temporal estricta.
2. **Protección Anti-Replay**:
   - El Relay mantiene un registro de `nonce` utilizados y rechaza inmediatamente con `TOKEN_REPLAY_DETECTED` cualquier intento de reutilizar un token interceptado o duplicado.
3. **Revocación Inmediata (`RevokeSession`)**:
   - Si el propietario del equipo anfitrión o el administrador revoca la sesión, el Relay invalida `session_id` en su tabla de sesiones revocadas y descarta cualquier stream pendiente o futuro.
4. **Privacidad de Datos**:
   - El Relay **no almacena** ningún paquete USB ni payload de transferencia en disco o base de datos; únicamente registra contadores agregados (`urbs_completed`, `bytes_out_to_device`, `bytes_in_from_device`).

---

## 3. Parámetros QUIC para Operaciones de Flasheo Prolongadas (Samsung Odin / EDL)

- **`keep_alive_interval = 5s`**: Mantiene vivas las tablas NAT de los routers domésticos mientras el teléfono borra particiones grandes (`super.img`, `userdata`), operación durante la cual el bus USB puede permanecer sin transferir paquetes durante 15–25 segundos.
- **`max_idle_timeout = 90s`**: Superior al timeout de URB de 60 segundos configurado para `SamsungDownload`.
- **`stream_receive_window = 16 MiB` / `receive_window = 32 MiB`**: Permite alto rendimiento en transferencias Bulk OUT sobre enlaces WAN con latencia sin desbordar la memoria del servidor Relay ni del Agente.

