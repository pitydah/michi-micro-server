# Michi Micro Server — Distributed Playback / Perch / PawPass Integration
## Complete Implementation Bible for OpenCode / Antigravity

**Target repository:** `pitydah/michi-micro-server`  
**Target branch:** current default branch (`main` at planning time)  
**Date:** 2026-09-24  
**Depends on:** Michi Link `authority-v1` + existing frozen `receiver-v1-lite`  
**Primary objective:** make Micro Server a first-class playback authority and handoff target/source for Michi Music Stream while preserving the existing receiver stack, fixing the liveness/discovery gaps that would make handoff unsafe, and reusing the current PlaybackEngine, ReceiverAudioSink, ReceiverSessionManager and RTP transport.

---

# 0. EXECUTION CONTRACT

This file is intended to be placed directly into the repository and followed by a coding agent.

Mandatory behavior:

1. Read this entire file before editing.
2. Inspect the live source tree and current tests.
3. Run the full existing test gate before changes.
4. Preserve all currently working receiver-v1-lite behavior.
5. Do not replace the existing receiver stack merely to introduce Perch/PawPass.
6. Do not introduce a second receiver protocol.
7. Do not introduce PipeWire as a Micro Server requirement.
8. Do not describe Micro Server playback as “bit-perfect”; that is not a Micro Server requirement.
9. Do not weaken tests, feature gates or evidence-level checks.
10. Implement authority as composition around existing session/RTP components.
11. Treat the canonical Michi Link vendored contract as the source of truth.
12. Any legacy handoff implementation should be migrated/adapted, not duplicated indefinitely.
13. Network liveness must be evidence-based; registry presence is not equivalent to “connected”.
14. Successful UDP `send()` is not proof the Stream is reproducing.
15. Final CI must include old receiver tests plus new distributed-authority tests.

---

# 1. BASELINE — WHAT ALREADY EXISTS

At planning time Micro Server already contains the important foundation:

```text
crates/michi-link/
crates/michi-connect/
crates/michi-receivers/
crates/michi-playback/
crates/michi-sync/
crates/michi-api/
crates/michi-rooms/
```

Relevant concrete code already exists:

```text
crates/michi-receivers/src/client.rs
crates/michi-receivers/src/session_manager.rs
crates/michi-receivers/src/transport.rs
crates/michi-receivers/src/models.rs

crates/michi-api/src/output/receiver_sink.rs
crates/michi-api/src/routes/v1/receivers.rs
crates/michi-api/src/routes/v1/playback.rs
crates/michi-api/src/sync_api.rs
crates/michi-api/src/server_caps.rs

crates/michi-connect/src/lib.rs
crates/michi-sync/src/lib.rs
```

Current receiver path is real:

```text
PlaybackEngine
    |
ReceiverAudioSink
    |
ReceiverSessionManager
    |
ReceiverClient
    |
POST /api/v1/receiver-lite/session
    |
RtpReceiverTransport
    |
Music Stream
```

Do not discard that architecture.

---

# 2. CURRENT STRENGTHS TO REUSE

The existing Micro code already provides:

- receiver pairing
- secure credential persistence
- receiver capability parsing
- session start
- session stop
- heartbeat
- negotiated Stream UDP port
- RTP packetization
- PCM S16LE baseline
- PlaybackEngine sink integration
- output target selection
- receiver simulator tests
- production ReceiverAudioSink tests
- Snapcast support separately from native receiver path
- sync state with queue/position/repeat/shuffle concepts
- SHA-256 in the sync subsystem
- playback session API

The new integration should leverage these assets.

---

# 3. CURRENT GAPS THAT MUST BE FIXED AS PART OF THIS WORK

Authority/handoff will be unreliable if these existing gaps remain.

## 3.1 Discovery is one-shot, not persistent

Current API discovery performs a short mDNS browse.

Needed:

```text
persistent signed Whisker discovery
+ Scent presence store
+ 90-second canonical expiry
+ identity/endpoint binding
```

## 3.2 Signed announce verification is not driving receiver presence

Michi Link already defines signatures/replay/timestamp validation. Micro must consume that evidence.

## 3.3 Heartbeat failure can leave a zombie session

A background heartbeat failure must become a state transition, not only a warning.

## 3.4 Receiver health is weaker than actual liveness

`paired == true` is not `healthy == true`.

## 3.5 Persisted capability restore must not invent capabilities

Restore exact last-known evidence with stale/unverified marking, then revalidate on Scent/server-info.

## 3.6 `receivers_connected` must not be registry length

Use a live/verified count or rename the metric.

## 3.7 UI pairing DTO mismatch

The web client and backend pairing DTOs must agree.

## 3.8 Pause/resume must propagate to receiver

When Stream session is active, session PATCH should reflect paused state.

## 3.9 Endpoint parsing must be URL-safe

Do not derive host with string splitting that breaks IPv6.

These repairs belong in the implementation because ownership transfer depends on truthful state.

---

# 4. NEW HIGH-LEVEL ARCHITECTURE

Add a distributed-authority layer in front of the existing receiver stack:

```text
                Michi Link Whisker
                       |
                Scent Presence
                       |
               Perch Authority Client
                       |
                PawPass Coordinator
                       |
              existing output resolver
                       |
              ReceiverAudioSink
                       |
           ReceiverSessionManager
                       |
          existing receiver-v1-lite
                       |
              existing RTP
```

Micro remains a playback host. It does not become a receiver arbiter; the physical Stream is the arbiter.

---

# 5. CAT SEMANTICS MAPPING IN MICRO

Use these names in docs/events/classes where useful, without renaming stable existing APIs:

```text
Whisker   = discovery service
Scent     = verified dynamic presence
Perch     = remote Stream authority state
PawPass   = cooperative handoff
Pounce    = explicit takeover
TailSync  = playback transfer payload
Purr      = existing receiver session, semantic only
Purrbeat  = existing heartbeat, semantic only
NineLives = recovery coordinator
```

Do not rename:

```text
ReceiverSessionManager
ReceiverAudioSink
RtpReceiverTransport
```

---

# 6. NEW MODULES / FILES

Recommended changes after confirming current tree.

## `crates/michi-connect`

Add:

```text
src/discovery_listener.rs
src/scent_store.rs
src/mdns_resolver.rs
```

Purpose:

- persistent multicast receive
- signed announcement verification
- mDNS endpoint resolution
- server/info identity reconciliation
- Scent expiry

## `crates/michi-receivers`

Add:

```text
src/authority_client.rs
src/authority_models.rs
src/authority_gate.rs
src/discovery_bridge.rs
src/session_supervisor.rs
```

Purpose:

- authority HTTP client
- authority grant storage in RAM
- session-start headers
- mapping Scent -> ReceiverRegistry
- heartbeat/session failure escalation

## `crates/michi-sync`

Extend or add:

```text
src/playback_transfer.rs
src/content_identity.rs
```

If this crate is still a single `lib.rs`, refactor only enough to add coherent modules; do not perform an unrelated rewrite.

## `crates/michi-api`

Add:

```text
src/routes/v1/playback_transfer.rs
```

or the current route-layout equivalent.

Expose PawPass target endpoints.

---

# 7. CONTRACT VENDORING

Pin `authority-v1` independently of `receiver-v1-lite`.

Expected metadata:

```text
authority profile version
bundle SHA-256
receiver-v1-lite version/hash
```

CI must detect drift.

Never edit downstream contract copies to “make tests pass”. Update through canonical Michi Link release workflow.

---

# 8. WHISKER — PERSISTENT DISCOVERY

Micro currently announces itself. Extend the same connectivity area with a persistent listener.

Canonical network:

```text
224.0.0.167:53318
```

Use existing `michi_identity::DiscoveryEngine` or canonical equivalent to validate:

- Ed25519 signature
- BLAKE3-derived Michi ID
- timestamp freshness
- replay nonce
- role/service profile
- all-or-nothing identity group

Pseudo service:

```rust
pub struct WhiskerDiscoveryService {
    socket: tokio::net::UdpSocket,
    engine: michi_identity::DiscoveryEngine,
    scent: Arc<ScentStore>,
    shutdown: CancellationToken,
}
```

Loop:

```rust
loop {
    tokio::select! {
        _ = shutdown.cancelled() => break,
        recv = socket.recv_from(&mut buf) => {
            let (n, source) = recv?;
            let packet = &buf[..n];

            match engine.observe(packet, source, Utc::now()) {
                Ok(verified) => scent.observe_signed(verified).await,
                Err(err) => metrics.discovery_rejected(err),
            }
        }
    }
}
```

Use the exact canonical API exposed by the vendored identity crate; the names above are illustrative.

---

# 9. SCENT STORE

Create a stable identity-based store.

```rust
#[derive(Debug, Clone)]
pub struct ScentRecord {
    pub michi_id: String,
    pub service: String,
    pub roles: Vec<String>,
    pub verified: bool,
    pub endpoints: Vec<SocketAddr>,
    pub base_url: Option<Url>,
    pub last_signed_seen: Instant,
    pub last_mdns_seen: Option<Instant>,
    pub server_info_verified_at: Option<Instant>,
    pub online: bool,
}
```

Key:

```text
michi_id
```

Never:

```text
friendly name
IP address
mDNS instance name
```

Canonical expiration:

```text
90 seconds without valid signed presence => offline
```

The store should emit typed events:

```rust
pub enum ScentEvent {
    Discovered(ScentRecord),
    Updated(ScentRecord),
    EndpointChanged { michi_id: String, old: Option<Url>, new: Url },
    Offline { michi_id: String },
}
```

---

# 10. MDNS AS ENDPOINT RESOLVER, NOT IDENTITY

Use current `mdns-sd` integration to resolve `_michi-link._tcp.local`.

When mDNS resolves a service:

1. collect candidate addresses/port
2. correlate with signed announcement
3. call `GET /api/v1/server/info`
4. require returned identity == signed `michi_id`
5. only then update trusted `base_url`

If hostname `.local` is not reliably resolvable in a container, prefer a resolved IP while preserving host-coherence checks.

Use `url::Url`, not:

```rust
trim_start_matches("http://").split(':')
```

---

# 11. DISCOVERY FILTER FOR MUSIC STREAM

Only create receiver projections for Stream profiles:

```text
service = michi-stream-standard OR michi-stream-hifi
api_version = v1-lite
role includes canonical audio_receiver constraints
```

Do not treat any `_michi-link._tcp` peer as a receiver.

---

# 12. RECEIVER REGISTRY MODEL REPAIR

Separate:

```text
stable identity
trust
endpoint
presence
capabilities
pairing credentials
active session
```

Suggested model evolution:

```rust
pub struct ReceiverRegistryEntry {
    pub michi_id: String,
    pub display_name: String,

    pub paired: bool,
    pub credential_ref: Option<String>,

    pub base_url: Option<Url>,
    pub presence: ReceiverPresence,

    pub capabilities: ReceiverCapabilities,
    pub capabilities_verified_at: Option<DateTime<Utc>>,
    pub capabilities_stale: bool,

    pub last_seen: Option<DateTime<Utc>>,
}
```

Presence:

```rust
pub enum ReceiverPresence {
    Unknown,
    Offline,
    VerifiedOnline,
    Degraded,
}
```

Do not reconstruct capabilities with made-up defaults after restart.

---

# 13. CAPABILITY PERSISTENCE

Persist the last verified `server/info` receiver capability set.

If schema migration is needed, store:

```text
supported_transports
supported_codecs
supported_sample_rates
supported_bit_depths
channels
features
authority features
observed_at
server_info_identity
```

At boot:

```text
load persisted capability evidence
mark stale = true
```

On verified Scent + `server/info`:

```text
replace with current evidence
stale = false
```

Do not add 44.1 kHz if Stream only certifies 48 kHz.

---

# 14. CONNECTED METRIC TRUTH

Current runtime field named `receivers_connected` must count live receiver evidence, not registry entries.

Preferred:

```rust
let connected = registry
    .list()
    .iter()
    .filter(|r| r.presence == ReceiverPresence::VerifiedOnline)
    .count();
```

Optionally also expose:

```text
receivers_registered
receivers_paired
receivers_online
receivers_streaming
```

This makes diagnostic state useful for authority/handoff.

---

# 15. AUTHORITY MODELS

Mirror canonical `authority-v1`.

Recommended:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorityStamp {
    pub authority_instance_id: String,
    pub lease_epoch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorityGrant {
    pub authority_instance_id: String,
    pub lease_epoch: u64,
    pub grant_token: String,
    pub activation_expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorityState {
    pub profile: String,
    pub state: String,
    pub receiver_michi_id: String,
    pub authority_instance_id: String,
    pub lease_epoch: u64,
    pub owner_michi_id: Option<String>,
    pub owner_service: Option<String>,
    pub owner_name: Option<String>,
    pub active_session_id: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub revision: u64,
}
```

Do not log `grant_token`.

---

# 16. AUTHORITY CLIENT

Add to `michi-receivers`.

```rust
pub struct ReceiverAuthorityClient {
    http: reqwest::Client,
}
```

Methods:

```rust
async fn info(&self, endpoint: &Url, token: &str) -> Result<AuthorityInfo, AuthorityError>;
async fn state(&self, endpoint: &Url, token: &str) -> Result<AuthorityState, AuthorityError>;
async fn claim(&self, ...) -> Result<AuthorityGrant, AuthorityError>;
async fn release(&self, ...) -> Result<AuthorityState, AuthorityError>;
async fn takeover(&self, ...) -> Result<AuthorityGrant, AuthorityError>;
async fn handoff(&self, ...) -> Result<AuthorityGrant, AuthorityError>;
```

Reuse the existing ReceiverClient HTTP/client stack if it already owns auth, TLS policy, timeouts and error mapping. Prefer adding methods to that client over adding a second HTTP implementation.

---

# 17. FEATURE PROBE

When receiver is verified online:

```text
GET server/info
if authority_v1 advertised:
    GET authority/info
else:
    LegacyAuthorityMode
```

Cache result for current Scent generation.

Do not keep authority support forever if firmware changed; revalidate after receiver reboot/version change.

---

# 18. STARTING A STREAM WITH PERCH

Existing `ReceiverSessionManager::start_session` becomes authority-aware without changing its public purpose.

Recommended high-level algorithm:

```rust
pub async fn start_session(...) -> Result<ActiveReceiverSession, ReceiverError> {
    let receiver = registry.require_verified(receiver_id)?;

    let authority = if receiver.supports_authority_v1() {
        Some(authority_gate.ensure_claim(receiver).await?)
    } else {
        None
    };

    let request = build_existing_receiver_lite_request(...)?;

    let created = client
        .session_start_with_authority(
            receiver.base_url,
            request,
            authority.as_ref(),
        )
        .await?;

    // existing RTP transport creation remains here
    ...
}
```

For authority-aware session creation, send the optional authority headers defined in `authority-v1`.

Legacy receiver path remains unchanged.

---

# 19. DO NOT CLAIM TOO EARLY

Perch grant activation TTL is short.

Therefore:

1. validate local track/output/capabilities first
2. prepare decoder enough to know source is openable
3. only then claim Perch
4. immediately create receiver session

Do not claim while scanning library or doing long disk work.

---

# 20. PAWPASS TARGET — PLAYER -> MICRO

Micro must expose:

```text
POST /api/v1/playback-transfer/prepare
POST /api/v1/playback-transfer/commit
POST /api/v1/playback-transfer/abort
```

Use canonical schemas.

---

# 21. TAILSYNC MODEL IN MICRO

Reuse current sync concepts where possible.

At planning time `michi-sync` already has:

```rust
PlaybackState {
    track_id
    position_ms
    playing
    volume
    playlist_id
    queue_position
    shuffle
    repeat
    event_id
    sequence
    epoch
    boot_id
    lamport
}
```

and `SessionData`.

Do not throw this away.

Add a protocol adapter:

```rust
impl TryFrom<TailSyncV1> for PreparedPlaybackTransfer { ... }
```

TailSync volume is not authoritative for Stream output in v1.

---

# 22. CONTENT IDENTITY

The sync subsystem already uses SHA-256 for uploaded files. Reuse that concept.

Canonical target resolver:

```rust
pub trait ContentIdentityResolver {
    async fn resolve(&self, content: &ContentRefV1)
        -> Result<ResolvedTrack, ContentResolutionError>;
}
```

Resolution order:

1. source-device/catalog mapping
2. `content_sha256`
3. sync mapping table
4. unique title+artist+duration within ±3000 ms
5. fail ambiguous/unresolved

Never automatically select the first fuzzy search result.

---

# 23. CONTENT HASH STORAGE

If track catalog does not currently persist whole-file SHA-256, do not hash every library file synchronously at startup.

Implement lazy hash cache:

```text
track_id
path identity
size
mtime_ns
sha256
computed_at
```

Hash on:

- sync/import where already calculated
- PawPass preparation if required
- idle/background enrichment where appropriate

Invalidate when size/mtime/path identity changes.

Use `spawn_blocking` for file hashing.

---

# 24. PREPARE ENDPOINT ALGORITHM

Pseudo:

```rust
async fn prepare_transfer(
    auth: PeerIdentity,
    body: PlaybackTransferPrepareRequest,
    state: AppState,
) -> Result<Json<PrepareResponse>, ApiError> {
    validate_tail_sync_bounds(&body.tailsync)?;

    let receiver = state
        .receiver_registry
        .get(&body.receiver.michi_id)
        .ok_or(CONTENT_OR_RECEIVER_ERROR)?;

    require_paired_receiver(receiver)?;
    require_verified_or_explicit_revalidation(receiver).await?;

    let prepared_queue = resolve_all_content_refs(&body.tailsync.queue).await?;
    let current = resolve_current(&body.tailsync.current).await?;

    // Validate playback engine can access current source.
    validate_media_openable(&current).await?;

    // Do NOT start RTP or claim Perch here.
    let pending = PendingPlaybackTransfer::new(...);
    state.pending_transfers.insert(pending)?;

    let ready_proof = state.identity.sign_pawpass_ready(...)?;

    Ok(ready(...))
}
```

Pending transfer TTL: contract default around 15 seconds.

Store in RAM, not durable DB.

---

# 25. PREPARE MUST NOT DESTROY CURRENT MICRO PLAYBACK

Micro might already be playing another zone/output.

Preparing a PawPass must not stop current playback before authority commit.

If current architecture supports only one playback engine/output at a time and target handoff would replace it, `prepare` must report readiness only if it can transition safely on commit.

Do not silently interrupt an unrelated active output.

If product policy says Micro is single-playback-host, return explicit conflict and let UI ask user.

---

# 26. READY PROOF

Use Micro's Michi identity manager.

Sign canonical target proof from authority-v1.

Do not create a custom RSA/HMAC scheme.

Proof must include:

```text
transfer_id
receiver_michi_id
source_michi_id
target_michi_id = Micro michi_id
authority instance
expected epoch
expiry
nonce
```

---

# 27. COMMIT ENDPOINT

The source app sends the Stream-issued target `AuthorityGrant`.

Algorithm:

1. authenticate source
2. load pending transfer by `transfer_id`
3. verify not expired
4. verify source identity matches prepare
5. query Stream `/authority/state`
6. require owner == Micro Michi ID
7. require instance/epoch match grant
8. start existing ReceiverLite session with grant headers
9. bind existing `ReceiverAudioSink`
10. install queue/current track
11. seek to TailSync position
12. apply repeat/shuffle
13. start/resume according to TailSync status
14. mark pending transfer committed
15. respond success

If session start fails:

- do not claim playback success
- best effort release target Perch using grant/current stamp
- mark transfer failed
- preserve honest diagnostics

---

# 28. PLAYBACK ENGINE INTEGRATION

Do not create a parallel “handoff playback engine”.

Use current:

```text
PlaybackEngine
resolve_output()
ReceiverAudioSink
```

The commit path should call application-level playback APIs similarly to current `/playback/session` behavior.

Refactor shared logic if needed so REST endpoints do not duplicate database queue mutation.

Recommended internal service:

```rust
pub struct PlaybackSessionApplicationService {
    ...
}

impl PlaybackSessionApplicationService {
    pub async fn apply_transfer(
        &self,
        prepared: PreparedPlaybackTransfer,
        output: PlaybackOutputSelection,
        position_ms: u64,
        status: TransferPlaybackStatus,
    ) -> Result<(), PlaybackError>;
}
```

Current route handler and PawPass commit can share it.

---

# 29. MICRO AS PAWPASS SOURCE

Micro must also transfer playback to Player/Mobile.

Create:

```rust
pub struct PawPassCoordinator {
    identity: IdentityManager,
    discovery: WhiskerDiscoveryService,
    receivers: ReceiverRegistry,
    playback: PlaybackEngine,
    ...
}
```

Method:

```rust
pub async fn transfer_to(
    &self,
    target_michi_id: &str,
    receiver_michi_id: &str,
) -> Result<TransferReceipt, PawPassError>;
```

Flow:

1. snapshot current playback truth
2. snapshot queue/current index
3. build TailSync
4. query Stream authority state
5. require Micro current owner
6. resolve target Scent/API endpoint
7. POST target `/playback-transfer/prepare`
8. verify ready response / target proof identity
9. POST Stream `/authority/handoff`
10. receive target grant
11. POST target `/playback-transfer/commit`
12. only after target commit success, transition Micro playback to transferred/stopped
13. if target commit fails after Stream ownership moved, mark output lost and surface recovery; do not keep sending stale RTP

The Stream revocation makes stale Micro audio impossible regardless of coordinator bugs.

---

# 30. MICRO AFTER PLAYER POUNCE

No direct callback is required for correctness.

When Player Pounces:

```text
Stream revokes Micro session
```

Micro detects through heartbeat.

The heartbeat supervisor MUST turn that into a terminal session loss.

---

# 31. HEARTBEAT SUPERVISOR — REQUIRED REPAIR

Current “log warning and continue” behavior is unsafe.

Create/extend `session_supervisor.rs`.

Track consecutive and terminal failures.

Pseudo:

```rust
enum HeartbeatDisposition {
    Continue,
    RetryTransient,
    SessionLost(SessionLossReason),
}

fn classify_heartbeat_error(err: &ReceiverClientError) -> HeartbeatDisposition {
    match err {
        Unauthorized |
        AuthorityRevoked |
        SessionNotFound |
        SessionConflict => SessionLost(...),

        Timeout |
        Io(_) => RetryTransient,

        _ => ...
    }
}
```

Policy:

- terminal authority/session error: immediate teardown
- transient network error: bounded consecutive threshold, e.g. 3 failures or lease deadline evidence
- never retry beyond known session lease as though still valid
- on lost:
  - cancel heartbeat task
  - stop/drop RTP transport
  - remove active session
  - mark sink unhealthy
  - notify PlaybackEngine/output projection
  - do not report Playing-to-Stream

---

# 32. HEARTBEAT TASK LIFECYCLE

Store task handle and cancellation token in active session state.

On:

```text
stop
PawPass away
Pounce detected
receiver offline
application shutdown
```

cancel and join task with bounded timeout.

Do not leak detached Tokio heartbeat loops.

---

# 33. RECEIVER SINK HEALTH

Current health cannot be:

```text
registry entry exists && paired
```

Health should include active session evidence.

Suggested:

```rust
pub struct ReceiverSinkHealth {
    pub paired: bool,
    pub receiver_online: bool,
    pub authority_owned: bool,
    pub session_active: bool,
    pub heartbeat_age: Option<Duration>,
    pub transport_open: bool,
    pub last_error: Option<String>,
}
```

`AudioSink::health()` should become unhealthy when session lease/liveness is lost.

If trait only returns bool/string today, build this internally and map truthfully.

---

# 34. PLAYBACK STATE TRUTH

Do not set global playback state to “remote playing” merely because RTP socket accepted bytes.

Evidence hierarchy:

```text
track decoder active
AND
ReceiverSession established
AND
Perch owner == Micro (authority-aware receiver)
AND
heartbeat fresh
AND
transport has no terminal error
```

Remote receiver metrics may enhance truth later.

---

# 35. PAUSE / RESUME

When active sink is a ReceiverAudioSink:

```text
pause:
    playback engine pauses decoding
    PATCH /receiver-lite/session {"paused": true}

resume:
    PATCH {"paused": false}
    playback engine resumes
```

Choose an ordering that avoids remote state claiming resumed before data can flow.

Suggested resume:

1. validate live session
2. PATCH paused=false
3. resume decoder
4. if decoder resume fails, best effort PATCH paused=true

Pause:

1. pause local decode
2. PATCH paused=true
3. surface PATCH failure as degraded remote state

---

# 36. VOLUME

Keep existing receiver volume behavior.

PawPass TailSync v1 does not force source-host local volume onto receiver.

The new owner queries/uses Stream volume or current session policy.

Do not make a transfer unexpectedly change Living Room volume.

---

# 37. LEGACY AUTHORITY FALLBACK

If `/authority/info` unavailable:

```text
AuthorityMode::LegacyReceiver
```

Behavior:

- existing ReceiverLite session start
- first-session/current receiver semantics
- no guaranteed Pounce
- PawPass can be cooperative only: old owner stops/releases legacy session, target creates new
- UI/API must expose reduced guarantee if needed

Do not emulate Perch locally and pretend it is receiver-enforced.

---

# 38. POUNCE CLIENT API

Micro itself may need explicit takeover from another host via UI/API.

Add service method:

```rust
pub async fn takeover_receiver(
    &self,
    receiver_id: &str,
    user_explicit: bool,
) -> Result<AuthorityGrant, ReceiverError>;
```

Reject call unless `user_explicit`.

Do not allow background auto-reconnect to Pounce another active owner.

Nine Lives auto-reconnect may reclaim only if:

- receiver is FREE, or
- receiver still says Micro is owner

Never auto-Pounce.

---

# 39. DISCOVERY API EVOLUTION

Keep current:

```text
POST /api/v1/devices/discover
```

for compatibility if clients use it.

Change its implementation to:

- request an immediate mDNS refresh
- return current Whisker/Scent snapshot
- do not create an isolated 3-second discovery universe

Add if useful:

```text
GET /api/v1/devices
```

or existing receiver list enriched with:

```json
{
  "presence": "verified_online",
  "last_seen": "...",
  "authority": {
    "supported": true,
    "state": "active",
    "owner_michi_id": "..."
  }
}
```

Do not expose secrets.

---

# 40. UI PAIRING DTO FIX

Backend current contract:

```text
start -> base_url (+ initiator_id)
confirm -> pairing_id + pin
```

Frontend must send those fields.

Do not keep:

```json
{"receiver_id":"..."}
```

if backend does not accept it.

Discovery result should provide/derive a verified `base_url`.

UI flow:

```text
Discovered Stream
  -> Pair
  -> press physical receiver button
  -> POST pair/start(base_url)
  -> enter PIN shown on Stream
  -> POST pair/confirm(pairing_id,pin)
  -> paired registry
```

Add browser/UI tests.

---

# 41. ENDPOINT / URL FIX

Current code must use `url::Url`.

Helper:

```rust
fn receiver_host(endpoint: &Url) -> Result<String, ReceiverError> {
    endpoint
        .host_str()
        .map(ToOwned::to_owned)
        .ok_or(ReceiverError::InvalidEndpoint)
}
```

For RTP destination, use resolved SocketAddr from Scent/server-info where possible.

IPv6 requires brackets only when formatting URLs, not when manually splitting strings.

---

# 42. PERSISTENT CREDENTIALS VS DYNAMIC ENDPOINT

Never key credential storage by URL alone.

Correct:

```text
michi_id -> credential
michi_id -> current Scent endpoint
```

When DHCP changes Stream IP:

```text
same Michi ID
new verified endpoint
same pairing token
```

Update endpoint after verified discovery.

---

# 43. PAIRING / DISCOVERY IDENTITY BINDING

When user pairs a discovered receiver:

1. have a verified signed Whisker `michi_id`
2. start pairing at resolved endpoint
3. `server/info` and pair responses identify receiver
4. require identity equal to discovered `michi_id`
5. only then persist credential binding

Never pair by “whatever answered this IP” after discovery indicated a different identity.

---

# 44. EXISTING `michi-sync` HANDOFF SEMANTICS

At planning time `michi-sync` already contains:

```rust
SyncMessage::HandoffRequest
SyncMessage::HandoffAccept
SessionData
PlaybackState
Lamport/epoch/boot_id conflict metadata
```

Do not delete these blindly.

Migration strategy:

- `PlaybackState` remains useful for synchronization
- new TailSync adapter maps richer queue/content identity
- old `HandoffRequest/HandoffAccept` can become deprecated compatibility messages
- new authoritative handoff is PawPass + Stream Perch, not merely a peer message

Mark legacy message path as non-authoritative.

---

# 45. QUEUE TRANSACTION

PawPass commit must not partially replace queue and then fail.

Use DB transaction:

1. validate all content refs
2. construct target queue
3. begin transaction
4. write queue state/current index
5. commit
6. start playback using committed queue

If playback start fails after DB commit, state may show queue but stopped; this is acceptable if reported honestly. Do not roll DB to stale queue after external Perch moved unless existing session transaction supports it safely.

---

# 46. CURRENT TRACK SEEK

TailSync position is advisory within actual track duration.

Clamp:

```text
0 <= position_ms <= duration_ms
```

If source/target duration differs materially:

- do not seek beyond target duration
- log content resolution evidence
- if weak metadata match and duration mismatch > 3s, fail prepare

---

# 47. PLAYBACK STATUS ON COMMIT

For TailSync:

```text
playing -> start at position
paused  -> load/seek, remain paused
stopped -> install queue/state but do not start remote session unless product semantics require ownership
```

For `stopped`, generally do NOT take Perch; PawPass is meaningful for active/paused playback. Validate product decision.

Recommended v1: PawPass only `playing|paused`.

---

# 48. SOURCE SNAPSHOT CONSISTENCY

Build TailSync from one coherent playback snapshot.

Do not separately read:

```text
track at T1
position at T2
queue at T3
```

if state can change between reads.

Use PlaybackEngine/queue revision and retry if revision changes during snapshot.

---

# 49. PREPARE RACE

If source playback moves to next track after target prepared but before handoff:

- source should compare snapshot revision before Stream handoff
- if changed, abort pending transfer and prepare again

Include `source_revision` in transfer context.

---

# 50. AUTHORITY EVENT PROJECTION

Expose to API/UI:

```rust
pub struct ReceiverAuthorityProjection {
    pub supported: bool,
    pub state: String,
    pub owner_michi_id: Option<String>,
    pub owner_name: Option<String>,
    pub is_owned_by_this_micro: bool,
    pub lease_epoch: Option<u64>,
}
```

Never expose grant tokens.

---

# 51. HTTP API FOR MICRO PAWPASS

Canonical endpoints:

```text
POST /api/v1/playback-transfer/prepare
POST /api/v1/playback-transfer/commit
POST /api/v1/playback-transfer/abort
```

Optional source action:

```text
POST /api/v1/playback-transfer/to/{target_michi_id}
```

If added, this is a convenience orchestration route, not a separate protocol.

---

# 52. AUTH OF PEER TRANSFER ENDPOINTS

Playback-transfer endpoints require trusted Michi peers.

Reuse current full-v1 device tokens/pairing.

The authenticated source Pawprint must match ready-proof source id.

Do not accept anonymous TailSync pushes.

---

# 53. BODY BOUNDS

Micro can handle more memory than Stream but must still bound:

```text
TailSync request <= 4 MiB
queue entries <= 10,000
title/artist/album reasonable max lengths
IDs reasonable max lengths
```

Reject before expensive content-resolution loops.

---

# 54. RATE LIMITS

Apply per paired peer:

```text
prepare <= 20/min
commit <= 20/min
abort <= 40/min
or existing API rate policy
```

Pounce/authority mutation to Stream is separately rate-limited at receiver.

---

# 55. NINE LIVES RECOVERY

## Receiver disappears during active playback

- Whisker Scent may eventually mark offline
- heartbeat/session supervisor should detect much faster
- stop remote transport
- playback projection -> lost/degraded
- do not auto-Pounce when it returns

## Receiver returns

- signed identity verification
- endpoint update
- server/info refresh
- if Stream says FREE and user resumes, claim/start new session
- if another owner exists, show in-use state

## Micro restarts

- no active ReceiverLite session assumed
- persisted receiver pairing survives
- capabilities loaded stale
- Whisker revalidates endpoint/capabilities
- playback session recovery never reuses old session token/SSRC

---

# 56. SNAPCAST / CLOWDER

Micro already integrates Snapcast. Preserve it.

Official policy:

```text
single receiver Direct:
    receiver-v1-lite + RTP

synchronized multiroom Clowder:
    Snapcast
```

Do not call existing concurrent native receiver fanout synchronized.

If current rooms/groups API fans out native sinks, label it honestly as fanout/routing unless timing evidence exists.

Future authority group work can compose Perch for individual receivers with Snapcast group routing.

---

# 57. PIPEWIRE / WIREPLUMBER

Do not make them Micro Server dependencies.

Micro Server is headless/container-oriented.

PipeWire/WirePlumber belong primarily to Music Player/local Linux graph integration.

---

# 58. GSTREAMER / ROC FUTURE TRANSPORT ADAPTER

Current native RTP works and has contract tests.

Do not replace it in this milestone.

Prepare an abstraction only if needed:

```rust
#[async_trait]
pub trait ReceiverTransport {
    async fn start(&mut self) -> Result<()>;
    async fn write_pcm(&mut self, data: &[u8]) -> Result<usize>;
    async fn stop(&mut self) -> Result<()>;
    fn health(&self) -> TransportHealth;
}
```

Current `RtpReceiverTransport` remains default.

Future:

```text
GStreamerRtpTransport
RocTransport
```

may be investigated behind feature flags.

Roc is not baseline because Stream/ESP32 compatibility and security profile must be proven first.

---

# 59. TEST LAYERS

## Pure unit

- authority models/error mapping
- ContentRef resolver
- TailSync validation
- Scent expiry
- endpoint rebinding
- heartbeat classification

## Integration

- Stream simulator authority endpoints
- legacy receiver flow
- authority-aware session start
- session revocation on Pounce
- PawPass prepare/commit
- persistent discovery simulation

## Cross-repo

- real Stream simulator from pinned contract
- Player/Mobile fixture clients where available

## Hardware

Only if physical Stream available.

---

# 60. WHISKER TESTS

Mandatory:

```text
valid_signed_stream_enters_scent
invalid_signature_rejected
stale_timestamp_rejected
replayed_nonce_rejected
friendly_name_collision_safe
mdns_without_signed_identity_not_trusted
server_info_identity_mismatch_rejected
endpoint_change_same_michi_id_updates_scent
no_announce_90s_marks_offline
offline_does_not_delete_pairing
```

---

# 61. SESSION SUPERVISOR TESTS

```text
heartbeat_authority_revoked_tears_down_immediately
heartbeat_404_tears_down
three_transient_timeouts_before_lease_mark_lost
no_retry_past_lease
task_cancelled_on_stop
task_joined_on_shutdown
udp_success_without_heartbeat_not_healthy
```

---

# 62. PAWPASS TARGET TESTS

```text
prepare_unpaired_source_rejected
prepare_unknown_receiver_rejected
prepare_unresolvable_track_rejected
prepare_ambiguous_content_rejected
prepare_valid_queue_returns_signed_proof
prepare_does_not_start_audio
commit_wrong_source_rejected
commit_wrong_grant_rejected
commit_stream_owner_not_micro_rejected
commit_creates_new_receiver_session
commit_seeks_position
commit_restores_repeat_shuffle
commit_playing_starts
commit_paused_remains_paused
abort_drops_pending_state
pending_transfer_expires
```

---

# 63. PAWPASS SOURCE TESTS

```text
source_snapshot_is_consistent
target_prepare_before_stream_handoff
target_not_ready_aborts
stream_handoff_before_target_commit
old_session_revoked
target_commit_success_stops_micro_old_playback
target_commit_failure_never_claims_success
source_revision_change_restarts_prepare
```

---

# 64. POUNCE LOSS TEST

Scenario:

```text
Micro -> Stream active
Player Pounces via simulator
Micro next heartbeat -> AUTHORITY_REVOKED
ReceiverSessionManager removes session
RtpReceiverTransport stops
PlaybackEngine projection not remote-playing
```

This is a release blocker.

---

# 65. UI TESTS

Backend web UI:

- discovery shows real endpoint, not “local” due wrong field
- pairing sends correct DTO
- owner state visible
- Take Over is explicit
- no auto-takeover on page load
- transfer menu only shows PawPass-capable peers
- errors surface exact conflict rather than generic success

---

# 66. SERVER CAPABILITY TRUTH

Update server capabilities carefully.

Micro may advertise:

```json
{
  "features": {
    "pawpass_v1": true,
    "tailsync_v1": true,
    "perch_client_v1": true
  }
}
```

Only after productive implementation exists and tests pass.

Do not mark hardware evidence from simulator-only CI.

---

# 67. DOCUMENTATION CLEANUP

Current receiver integration docs may still call implemented phases “prototype”.

After implementation:

- update status from code evidence
- separate `IntegrationCertified(simulator)` from physical-device certification
- explain authority-v1
- explain legacy fallback
- explain Direct vs Clowder

Do not inflate maturity labels.

---

# 68. DATABASE MIGRATION

If receiver table needs new columns:

```text
michi_id stable key
base_url nullable
presence not necessarily persisted
capabilities_json
capabilities_observed_at
authority_supported
```

Use numbered SQL migration.

Migration must be:

- idempotent under normal migration runner
- rollback/testable if repository supports down migrations
- preserve existing paired tokens
- not assign fabricated Michi IDs

If old rows only have legacy device id, retain legacy identifier until verified Scent maps it.

---

# 69. SHUTDOWN

AppState shutdown must cancel:

```text
Whisker listener
mDNS resolver
Scent expiry task
receiver heartbeat supervisors
pending transfer expiry worker
```

Join tasks.

No detached background loops after Axum/server shutdown.

---

# 70. OBSERVABILITY

Structured events:

```text
whisker.peer_verified
whisker.peer_rejected
scent.online
scent.offline
scent.endpoint_changed
perch.claimed
perch.occupied
pawpass.prepared
pawpass.committed
pawpass.failed
pounce.requested
purr.started
purr.revoked
purr.heartbeat_lost
nine_lives.rediscovered
```

Metrics:

```text
michi_receivers_registered
michi_receivers_online
michi_receiver_sessions_active
michi_receiver_heartbeat_failures_total
michi_pawpass_attempts_total
michi_pawpass_failures_total
michi_pounce_total
michi_discovery_signature_rejections_total
```

No secrets in labels.

---

# 71. SECURITY

Never log:

```text
device token
receiver session token
authority grant
private key
pairing PIN
```

Verify:

- HTTPS is not assumed on LAN; identity is cryptographic
- ready proof signature
- source peer auth
- receiver identity consistency
- bounded replay windows
- endpoint change after verified identity

A malicious mDNS answer alone must not redirect paired traffic.

---

# 72. FILE-BY-FILE CHANGE SUMMARY

## `crates/michi-connect/src/lib.rs`

- keep announce path
- export Whisker listener startup
- no monolithic God file after addition

## `crates/michi-connect/src/discovery_listener.rs` NEW

- signed UDP listener
- canonical verify
- lifecycle

## `crates/michi-connect/src/scent_store.rs` NEW

- presence store
- expiry
- events

## `crates/michi-receivers/src/client.rs`

- authority info/state/claim/release/takeover/handoff
- authority optional headers on ReceiverLite create
- use `Url`

## `crates/michi-receivers/src/session_manager.rs`

- authority gate before session
- supervisor
- terminal heartbeat handling
- task lifecycle
- pause/resume remote patch

## `crates/michi-receivers/src/transport.rs`

- no authority logic
- improve health semantics only as required
- keep RTP baseline

## `crates/michi-receivers/src/models.rs`

- stable identity/presence/capability evidence

## `crates/michi-api/src/output/receiver_sink.rs`

- session truth health
- propagate pause/resume
- no paired==healthy assumption

## `crates/michi-api/src/routes/v1/receivers.rs`

- discovery snapshot
- pairing DTO consistency
- authority state projection
- takeover/release API if exposed

## `crates/michi-api/src/routes/v1/playback_transfer.rs` NEW

- prepare/commit/abort

## `crates/michi-sync/src/playback_transfer.rs` NEW

- TailSync mapping
- pending transfer model

## `crates/michi-sync/src/content_identity.rs` NEW

- resolver/cache semantics

## `crates/michi-api/src/server_caps.rs`

- connected metrics truth
- feature evidence

## `crates/michi-api/static/app.js`

- pairing DTO fix
- owner UX
- explicit takeover/handoff

---

# 73. CARGO DEPENDENCIES

Prefer existing dependencies.

Likely already available:

```text
tokio
serde
serde_json
reqwest
url
uuid
chrono
sha2
tracing
thiserror
```

For multicast networking, use Tokio/std socket facilities already compatible with project.

Do not add a large framework if existing crates cover it.

Use canonical Michi identity crate for signatures.

---

# 74. PRE-FLIGHT COMMANDS

Use repository's actual commands. Typical:

```bash
git status --short
git rev-parse HEAD
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

Also run receiver simulator/E2E scripts already present.

Do not invent a command if Cargo/workflow defines a different gate; inspect CI first.

---

# 75. PHASES

## M0 — baseline + contract pin

No runtime change.

## M1 — Whisker/Scent

Persistent trusted receiver presence.

## M2 — receiver registry truth repair

Identity/endpoint/capability separation.

## M3 — session supervisor repair

Eliminate zombie remote-playing state.

## M4 — authority client

Claim/release/state/Pounce and authority session headers.

## M5 — PawPass target

Prepare/commit/abort + TailSync + content resolver.

## M6 — PawPass source

Transfer Micro playback to Player/Mobile.

## M7 — API/UI

Owner projection, explicit takeover, transfer UI, pairing fix.

## M8 — recovery

IP changes, restart, offline/online.

## M9 — cross-repo E2E

Pinned Stream simulator and authority vectors.

## M10 — docs/capability evidence

Only after implementation works.

---

# 76. PHASE GATES

Each phase:

```text
pre-existing tests green
new unit tests green
new integration tests green
no ignored new failures
git diff reviewed
```

Do not proceed from session supervisor to PawPass while zombie-session tests are red.

---

# 77. ROLLBACK RULE

If an authority integration change breaks legacy ReceiverLite:

- preserve old path
- isolate authority feature probe
- roll back incompatible change
- reintroduce through optional overlay

Do not patch the frozen Stream contract to fit Micro.

---

# 78. DEFINITION OF DONE

This repository is complete for distributed playback when:

- persistent signed Stream discovery exists
- Scent is keyed by `michi_id`
- 90-second presence truth is implemented
- mDNS endpoint is bound to signed/server-info identity
- pairing follows verified endpoint identity
- capabilities are persisted truthfully and revalidated
- `receivers_connected` no longer lies
- authority-v1 feature probing works
- Micro can Claim a free Stream
- Micro can Pounce only on explicit user command
- Micro can start ReceiverLite with authority grant
- heartbeat loss revokes local session truth
- old UDP write success cannot keep session “healthy”
- pause/resume reaches Stream session
- Player/Mobile can PawPass playback to Micro
- Micro can PawPass playback to another host
- TailSync queue/position/repeat/shuffle transfers
- content resolver never chooses ambiguous first result
- source and target never share a ReceiverLite session/token/SSRC
- legacy receivers remain usable
- Snapcast remains separate synchronized multiroom path
- old CI remains green
- new cross-repo scenarios pass

---

# 79. FINAL AGENT REPORT

Return:

```text
Baseline commit:
Authority contract bundle:
ReceiverLite contract bundle:
Database migrations:
Files added:
Files changed:
Existing defects corrected:
Unit tests:
Integration tests:
Simulator tests:
Cross-repo tests:
UI tests:
Clippy/fmt:
Hardware evidence:
Remaining external blockers:
```

Do not call simulator evidence “physical-device certification”.

---

# 80. OPEN/FREE TECHNOLOGY REFERENCES

Use these for implementation decisions, not as mandatory Micro runtime dependencies:

- Avahi/mDNS-DNS-SD: https://github.com/avahi/avahi
- GStreamer RTP: https://gstreamer.freedesktop.org/documentation/additional/rtp.html
- GStreamer jitterbuffer: https://gstreamer.freedesktop.org/documentation/rtpmanager/rtpjitterbuffer.html
- Snapcast: https://github.com/snapcast/snapcast
- PipeWire RTP reference: https://docs.pipewire.org/devel/page_module_rtp_session.html
- Roc Toolkit exploratory transport: https://github.com/roc-streaming/roc-toolkit
- RTP: RFC 3550
- mDNS: RFC 6762
- DNS-SD: RFC 6763

Micro Server remains a lightweight Rust/headless server. Linux desktop graph technologies must not become a hard requirement.
