# Arquitectura y Límites de Extracción: Michi Server Core (M9)

## 1. Contexto y Principio Rector
El prompt arquitectónico del ecosistema Michi establece:
> **CONTROL ≠ SERVER ≠ AUDIO ENDPOINT**  
> *Mobile controls. Michi Server orchestrates. Michi Server streams. Michi Stream plays.*

Michi Micro Server (`michi-micro-server`) es la **implementación de referencia** del concepto canónico `MichiServer`. En el futuro, un eventual `Michi Big Server` compartirá la capa de dominio y contratos sin reinventar protocolos ni estructuras de datos.

Este documento formaliza las fronteras arquitectónicas, la dirección de dependencias y los componentes candidatos a ser extraídos en un futuro crate `michi-server-core` **sin realizar la extracción física prematuramente**.

---

## 2. Diagrama de Capas Canónicas

```
┌─────────────────────────────────────────────────────────────┐
│                    Capas de Interfaz                        │
│   • michi-api (Axum HTTP/WS routes, REST endpoints)         │
│   • michi-tui (Terminal UI)                                 │
│   • michi-opensubsonic (Subsonic API adapter)               │
│   • michi-homeassistant (MQTT discovery & entities)         │
├─────────────────────────────────────────────────────────────┤
│               Capa de Contrato / Michi Link                 │
│   • michi-link                                              │
│     ├── roles (ServerRole, CANONICAL_MICRO_ROLES)           │
│     ├── events (LinkEvent: playback, queue, receiver)       │
│     ├── models (DTOs de emparejamiento, sesiones, etc.)     │
│     └── auth (TokenStore, DeviceRegistry)                   │
├─────────────────────────────────────────────────────────────┤
│                  Dominio (Server Core)                      │
│   • michi-core                                              │
│     ├── PlaybackTarget (Local, Receiver, Room)              │
│     ├── AudioEndpoint (hardware-agnostic sink)              │
│     ├── Zone (UX musical destination)                       │
│     ├── LibrarySource (storage abstraction)                 │
│     ├── Track / AudioFormat / ResourceProfile               │
│     └── PlaybackSessionDb                                   │
├─────────────────────────────────────────────────────────────┤
│             Orquestación y Gestión de Endpoints             │
│   • michi-receivers                                         │
│     ├── ReceiverManager (discovery, pairing, sessions)      │
│     ├── AudioTransport trait (RTP v1, ALSA, Snapcast)       │
│     └── Capability Negotiation (Server ∩ Receiver)          │
│   • michi-rooms (Zone / Room grouping)                      │
│   • michi-sync (Peer-to-peer sync engine)                   │
├─────────────────────────────────────────────────────────────┤
│               Infraestructura y Persistencia                │
│   • michi-db (SQLite pool, migrations)                      │
│   • michi-scanner (Async filesystem scanner)                │
│   • michi-streaming (HTTP Range audio streaming)            │
│   • michi-security (Rate limiting, idempotency)             │
│   • michi-identity (Ed25519 identity, Argon2id)             │
└─────────────────────────────────────────────────────────────┘
```

---

## 3. Dirección de Dependencias Estricta

Para preservar la pureza del dominio y permitir la extracción futura:

1. **`michi-core`** NO debe depender de ningún framework web (Axum, Tower), base de datos (SQLx), ni crates internos. Solo depende de `std`, `serde`, `uuid`, `chrono`, `thiserror`, `utoipa`.
2. **`michi-link`** define la norma del protocolo (especificación). Es autoritativo.
3. **`michi-receivers`** implementa la gestión de endpoints y define `AudioTransport`. No contiene UI ni lógica de biblioteca.
4. **`michi-api`** es únicamente una capa de transporte/enrutamiento que consume `michi-core`, `michi-link` y los crates de infraestructura.

---

## 4. Candidatos a Extracción para `Michi Server Core`

Cuando se decida formalizar `michi-server-core`, los siguientes módulos serán unificados:
- Modelos de dominio (`PlaybackTarget`, `AudioEndpoint`, `Zone`, `LibrarySource`) de `michi-core`.
- Abstracción `AudioTransport` y `ReceiverManager` de `michi-receivers`.
- Sistema de eventos canónicos `LinkEvent` de `michi-link`.
- Lógica de orquestación de sesiones de reproducción (`PlaybackSession`).
- Motor de negociación de capacidades (`ServerCaps ∩ ReceiverCaps`).
