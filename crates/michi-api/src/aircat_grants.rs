//! A28 cross-repository producer security kernel for michi-api (Rust).
//!
//! Integration target: crates/michi-api/src/routes/v1/aircat.rs, with an existing
//! HomeRootAuthority signing key and an independently authenticated Home member.
//! IMPORTANT: this is source code, NOT a live route, and has not passed cargo.
//! Callers MUST resolve allowed assets from their actual original files; never
//! derive `ApprovedAsset` from Mobile's untrusted manifest alone.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

const MAX_ITEMS: usize = 10_000;
const MAX_FIELD_BYTES: usize = 4096;
const MAX_GRANT_MS: i64 = 600_000;
fn default_version() -> i32 {
    1
}
fn default_selection() -> String {
    "ALBUMS".to_string()
}
fn default_scopes() -> Vec<String> {
    vec!["content:read".into(), "content:transfer".into()]
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileItem {
    pub logical_track_id: String,
    pub remote_track_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub format: String,
    pub size: i64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    #[serde(default = "default_version")]
    pub manifest_version: i32,
    pub transfer_id: String,
    pub source_device_id: String,
    pub total_tracks: i32,
    pub total_bytes: i64,
    pub items: Vec<FileItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Intent {
    pub transfer_id: String,
    pub source_michi_id: String,
    pub destination_michi_id: String,
    pub source_library_id: String,
    pub destination_library_id: String,
    #[serde(default = "default_selection")]
    pub selection_scope: String,
    #[serde(default = "default_version")]
    pub manifest_version: i32,
    #[serde(default = "default_scopes")]
    pub required_scopes: Vec<String>,
    pub expires_at: i64,
    #[serde(default)]
    pub signed_grant: Option<serde_json::Value>,
    #[serde(default)]
    pub authorization_nonce_base64_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub intent: Intent,
    pub manifest: Manifest,
    pub manifest_sha256: String,
    pub client_nonce_base64_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Grant {
    pub version: i32,
    pub home_id: String,
    pub authority_michi_id: String,
    pub transfer_id: String,
    pub source_michi_id: String,
    pub destination_michi_id: String,
    pub source_library_id: String,
    pub destination_library_id: String,
    pub selection_scope: String,
    pub scopes: Vec<String>,
    pub manifest_sha256: String,
    pub issued_at_millis: i64,
    pub expires_at_millis: i64,
    pub nonce_base64_url: String,
    pub signature_base64_url: String,
}

/// This structure is created by Home-authenticated server code, NOT from headers.
/// `approved_assets` must be populated by reading actual original file bytes.
pub struct AuthorizedContext {
    pub home_id: String,
    pub source_michi_id: String,
    pub destination_michi_id: String,
    pub source_library_id: String,
    pub allowed_destination_library_id: String,
    pub is_active_home_member: bool,
    pub may_read_original_files: bool,
    pub approved_assets: Vec<ApprovedAsset>,
}

#[derive(Clone)]
pub struct ApprovedAsset {
    pub remote_track_id: String,
    pub exact_size: i64,
    pub exact_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    InvalidFields,
    UntrustedHomeMember,
    NotOriginalBytes,
    Expired,
    WrongAudience,
    BadSignature,
    Revoked,
}

fn write_field(out: &mut Vec<u8>, value: &str) -> Result<(), Refusal> {
    let bytes = value.as_bytes();
    if bytes.len() > MAX_FIELD_BYTES {
        return Err(Refusal::InvalidFields);
    }
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

pub fn manifest_bytes(m: &Manifest) -> Result<Vec<u8>, Refusal> {
    if m.manifest_version != 1
        || m.items.is_empty()
        || m.items.len() > MAX_ITEMS
        || m.total_tracks as usize != m.items.len()
        || m.total_bytes <= 0
        || m.transfer_id.is_empty()
        || m.source_device_id.is_empty()
    {
        return Err(Refusal::InvalidFields);
    }
    let mut out = Vec::new();
    write_field(&mut out, "MICHI-AIRCAT-MANIFEST-V1")?;
    out.extend_from_slice(&m.manifest_version.to_be_bytes());
    write_field(&mut out, &m.transfer_id)?;
    write_field(&mut out, &m.source_device_id)?;
    out.extend_from_slice(&m.total_tracks.to_be_bytes());
    out.extend_from_slice(&m.total_bytes.to_be_bytes());
    out.extend_from_slice(&(m.items.len() as i32).to_be_bytes());
    let mut ids = HashSet::new();
    let mut total = 0i64;
    for f in &m.items {
        if f.size <= 0
            || f.format.is_empty()
            || f.remote_track_id.is_empty()
            || f.logical_track_id.is_empty()
            || !ids.insert(f.logical_track_id.as_str())
            || f.sha256.len() != 64
            || !f.sha256.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(Refusal::InvalidFields);
        }
        total = total.checked_add(f.size).ok_or(Refusal::InvalidFields)?;
        for field in [
            &f.logical_track_id,
            &f.remote_track_id,
            &f.title,
            &f.artist,
            &f.album,
            &f.format,
        ] {
            write_field(&mut out, field)?;
        }
        out.extend_from_slice(&f.size.to_be_bytes());
        write_field(&mut out, &f.sha256.to_ascii_lowercase())?;
    }
    if total != m.total_bytes {
        return Err(Refusal::InvalidFields);
    }
    Ok(out)
}

pub fn manifest_sha256(m: &Manifest) -> Result<String, Refusal> {
    Ok(hex::encode(Sha256::digest(manifest_bytes(m)?)))
}

pub fn grant_bytes(g: &Grant) -> Result<Vec<u8>, Refusal> {
    let mut out = Vec::new();
    write_field(&mut out, "MICHI-AIRCAT-GRANT-V1")?;
    out.extend_from_slice(&g.version.to_be_bytes());
    for v in [
        &g.home_id,
        &g.authority_michi_id,
        &g.transfer_id,
        &g.source_michi_id,
        &g.destination_michi_id,
        &g.source_library_id,
        &g.destination_library_id,
        &g.selection_scope,
    ] {
        write_field(&mut out, v)?;
    }
    out.extend_from_slice(&(g.scopes.len() as i32).to_be_bytes());
    for scope in &g.scopes {
        write_field(&mut out, scope)?;
    }
    write_field(&mut out, &g.manifest_sha256)?;
    out.extend_from_slice(&g.issued_at_millis.to_be_bytes());
    out.extend_from_slice(&g.expires_at_millis.to_be_bytes());
    write_field(&mut out, &g.nonce_base64_url)?;
    Ok(out)
}

/// Sign only after Home authentication, ACL verification and actual bytes match.
/// `issuer` is THE PERSISTENT Home root signing key, not a new AirCat identity.
pub fn issue_grant(
    request: &Request,
    ctx: &AuthorizedContext,
    issuer: &michi_identity::HomeRootAuthority,
    now_ms: i64,
) -> Result<Grant, Refusal> {
    if !ctx.is_active_home_member || !ctx.may_read_original_files {
        return Err(Refusal::UntrustedHomeMember);
    }
    let root_id = issuer.home_id();
    if ctx.home_id != root_id
        || request.intent.source_michi_id != ctx.source_michi_id
        || request.intent.destination_michi_id != ctx.destination_michi_id
        || request.intent.source_library_id != ctx.source_library_id
        || request.intent.destination_library_id != ctx.allowed_destination_library_id
        || request.intent.transfer_id != request.manifest.transfer_id
        || request.intent.source_michi_id != request.manifest.source_device_id
        || request.intent.manifest_version != request.manifest.manifest_version
        || request.intent.signed_grant.is_some()
        || !request.intent.authorization_nonce_base64_url.is_empty()
        || request.intent.expires_at <= now_ms
        || request.intent.selection_scope.is_empty()
    {
        return Err(Refusal::WrongAudience);
    }
    if request.manifest_sha256 != manifest_sha256(&request.manifest)? {
        return Err(Refusal::InvalidFields);
    }
    let nonce = URL_SAFE_NO_PAD
        .decode(&request.client_nonce_base64_url)
        .map_err(|_| Refusal::InvalidFields)?;
    if nonce.len() < 16
        || nonce.len() > 64
        || URL_SAFE_NO_PAD.encode(&nonce) != request.client_nonce_base64_url
    {
        return Err(Refusal::InvalidFields);
    }
    let mut unique = HashSet::new();
    for file in &request.manifest.items {
        let known = ctx
            .approved_assets
            .iter()
            .find(|a| a.remote_track_id == file.remote_track_id)
            .ok_or(Refusal::NotOriginalBytes)?;
        if known.exact_size != file.size
            || !known.exact_sha256.eq_ignore_ascii_case(&file.sha256)
            || !unique.insert(file.remote_track_id.as_str())
        {
            return Err(Refusal::NotOriginalBytes);
        }
    }
    let mut scopes = request.intent.required_scopes.clone();
    scopes.sort();
    scopes.dedup();
    // v1 does not delegate new privileges from a caller-controlled list.
    if scopes != ["content:read".to_string(), "content:transfer".to_string()] {
        return Err(Refusal::InvalidFields);
    }
    if now_ms <= 0 {
        return Err(Refusal::InvalidFields);
    }
    let until = now_ms
        .checked_add(MAX_GRANT_MS)
        .ok_or(Refusal::InvalidFields)?
        .min(request.intent.expires_at);
    let mut grant = Grant {
        version: 1,
        home_id: ctx.home_id.clone(),
        authority_michi_id: root_id,
        transfer_id: request.intent.transfer_id.clone(),
        source_michi_id: ctx.source_michi_id.clone(),
        destination_michi_id: ctx.destination_michi_id.clone(),
        source_library_id: ctx.source_library_id.clone(),
        destination_library_id: ctx.allowed_destination_library_id.clone(),
        selection_scope: request.intent.selection_scope.clone(),
        scopes,
        manifest_sha256: request.manifest_sha256.clone(),
        issued_at_millis: now_ms,
        expires_at_millis: until,
        nonce_base64_url: request.client_nonce_base64_url.clone(),
        signature_base64_url: String::new(),
    };
    // Sign with the unique persistent Home Root Authority. No caller receives
    // a reference to its private key, and the canonical byte layout is owned
    // by the normative michi-link contract, not this service.
    let claims = michi_identity::AirCatGrantClaims {
        version: grant.version,
        home_id: grant.home_id.clone(),
        authority_michi_id: grant.authority_michi_id.clone(),
        transfer_id: grant.transfer_id.clone(),
        source_michi_id: grant.source_michi_id.clone(),
        destination_michi_id: grant.destination_michi_id.clone(),
        source_library_id: grant.source_library_id.clone(),
        destination_library_id: grant.destination_library_id.clone(),
        selection_scope: grant.selection_scope.clone(),
        scopes: grant.scopes.clone(),
        manifest_sha256: grant.manifest_sha256.clone(),
        issued_at_millis: grant.issued_at_millis,
        expires_at_millis: grant.expires_at_millis,
        nonce_base64_url: grant.nonce_base64_url.clone(),
    };
    let signed = issuer
        .issue_aircat_grant_v1(&claims, now_ms)
        .map_err(|_| Refusal::InvalidFields)?;
    grant.signature_base64_url = signed.signature_base64_url;
    Ok(grant)
}

pub fn verify_grant(
    grant: &Grant,
    root_public_key: &VerifyingKey,
    current_destination_michi_id: &str,
    now_ms: i64,
    revoked: bool,
) -> Result<(), Refusal> {
    if revoked {
        return Err(Refusal::Revoked);
    }
    let home_id = URL_SAFE_NO_PAD.encode(blake3::hash(root_public_key.as_bytes()).as_bytes());
    if grant.home_id != home_id
        || grant.authority_michi_id != home_id
        || grant.destination_michi_id != current_destination_michi_id
        || grant.version != 1
    {
        return Err(Refusal::WrongAudience);
    }
    let future_tolerance = now_ms.checked_add(60_000).ok_or(Refusal::Expired)?;
    if grant.issued_at_millis <= 0
        || now_ms <= 0
        || now_ms >= grant.expires_at_millis
        || grant.issued_at_millis > future_tolerance
        || grant.expires_at_millis <= grant.issued_at_millis
        || grant.expires_at_millis - grant.issued_at_millis > MAX_GRANT_MS
    {
        return Err(Refusal::Expired);
    }
    // A29: delegated privileges cannot be enlarged by a validly signed but
    // overly permissive legacy grant. Scope order must match Mobile's verifier.
    if grant.scopes != vec!["content:read".to_string(), "content:transfer".to_string()]
        || grant.manifest_sha256.len() != 64
        || !grant
            .manifest_sha256
            .bytes()
            .all(|x| x.is_ascii_hexdigit() && !x.is_ascii_uppercase())
    {
        return Err(Refusal::InvalidFields);
    }
    let nonce = URL_SAFE_NO_PAD
        .decode(&grant.nonce_base64_url)
        .map_err(|_| Refusal::InvalidFields)?;
    if nonce.len() < 16
        || nonce.len() > 64
        || URL_SAFE_NO_PAD.encode(&nonce) != grant.nonce_base64_url
    {
        return Err(Refusal::InvalidFields);
    }
    let signature_bytes = URL_SAFE_NO_PAD
        .decode(&grant.signature_base64_url)
        .map_err(|_| Refusal::BadSignature)?;
    let signature = Signature::from_slice(&signature_bytes).map_err(|_| Refusal::BadSignature)?;
    root_public_key
        .verify(&grant_bytes(grant)?, &signature)
        .map_err(|_| Refusal::BadSignature)
}

/// A29: final per-GET authorization boundary. This function takes a PRODUCER-
/// CONTROLLED manifest snapshot and asset digest, not data taken from a GET
/// header. A route must first authenticate the request as `authenticated_member`;
/// it must load the recorded selection by transferId+manifestSha256, verify
/// Home revocations, and open the original file under configured music roots.
/// Returning Ok allows building the ACK for the same request; it does not
/// attest to TLS, atomicity or immutability of a file being read concurrently.
#[allow(clippy::too_many_arguments)]
pub fn authorize_original_asset_request(
    grant: &Grant,
    root_key: &VerifyingKey,
    authenticated_member: &str,
    home_ctx: &AuthorizedContext,
    retained_manifest: &Manifest,
    requested_remote_id: &str,
    original_file_size: i64,
    original_file_sha256: &str,
    now_ms: i64,
    revoked: bool,
) -> Result<(), Refusal> {
    if revoked {
        return Err(Refusal::Revoked);
    }
    if authenticated_member.is_empty()
        || !home_ctx.is_active_home_member
        || !home_ctx.may_read_original_files
        || home_ctx.destination_michi_id != authenticated_member
        || home_ctx.source_michi_id != grant.source_michi_id
        || home_ctx.home_id != grant.home_id
        || home_ctx.source_library_id != grant.source_library_id
        || home_ctx.allowed_destination_library_id != grant.destination_library_id
        || retained_manifest.transfer_id != grant.transfer_id
        || retained_manifest.source_device_id != grant.source_michi_id
        || grant.manifest_sha256 != manifest_sha256(retained_manifest)?
    {
        return Err(Refusal::WrongAudience);
    }
    verify_grant(grant, root_key, authenticated_member, now_ms, revoked)?;
    if requested_remote_id.is_empty() || original_file_size <= 0 || original_file_sha256.len() != 64
    {
        return Err(Refusal::NotOriginalBytes);
    }
    let selected: Vec<&FileItem> = retained_manifest
        .items
        .iter()
        .filter(|x| x.remote_track_id == requested_remote_id)
        .collect();
    // Never pick a duplicate/missing ID by first-match iteration.
    if selected.len() != 1 {
        return Err(Refusal::NotOriginalBytes);
    }
    let item = selected[0];
    let allowed: Vec<&ApprovedAsset> = home_ctx
        .approved_assets
        .iter()
        .filter(|x| x.remote_track_id == requested_remote_id)
        .collect();
    if allowed.len() != 1
        || item.size != original_file_size
        || allowed[0].exact_size != original_file_size
        || !item.sha256.eq_ignore_ascii_case(original_file_sha256)
        || !allowed[0]
            .exact_sha256
            .eq_ignore_ascii_case(original_file_sha256)
    {
        return Err(Refusal::NotOriginalBytes);
    }
    Ok(())
}

/// This SHA-256 header is only an acknowledgement; ACL/grant checks MUST have
/// succeeded first, on the same authenticated request and selected asset.
pub fn receipt_for_grant_header(encoded: &str) -> Result<String, Refusal> {
    if encoded.is_empty()
        || encoded.len() > 8192
        || !encoded
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(Refusal::InvalidFields);
    }
    Ok(hex::encode(Sha256::digest(encoded.as_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    #[test]
    fn python_kotlin_golden_vector_manifest() {
        let manifest = Manifest {
            manifest_version: 1,
            transfer_id: "tx".into(),
            source_device_id: "source".into(),
            total_tracks: 1,
            total_bytes: 6,
            items: vec![FileItem {
                logical_track_id: "local".into(),
                remote_track_id: "remote".into(),
                title: "song".into(),
                artist: "artist".into(),
                album: "album".into(),
                format: "flac".into(),
                size: 6,
                sha256: "a".repeat(64),
            }],
        };
        assert_eq!(
            manifest_sha256(&manifest).unwrap(),
            "4e053cfccaeda739685245d3e225e0c4b1e0cff66a4878a6fd1693e0d22074ea"
        );
    }
    #[test]
    fn wrong_total_never_canonicalized() {
        let m = Manifest {
            manifest_version: 1,
            transfer_id: "tx".into(),
            source_device_id: "source".into(),
            total_tracks: 1,
            total_bytes: 9,
            items: vec![FileItem {
                logical_track_id: "a".into(),
                remote_track_id: "b".into(),
                title: "".into(),
                artist: "".into(),
                album: "".into(),
                format: "flac".into(),
                size: 6,
                sha256: "a".repeat(64),
            }],
        };
        assert!(manifest_sha256(&m).is_err());
    }
    fn selected_asset_fixture() -> (Request, AuthorizedContext, SigningKey, Grant) {
        let signer = SigningKey::from_bytes(&[13u8; 32]);
        let home_id =
            URL_SAFE_NO_PAD.encode(blake3::hash(signer.verifying_key().as_bytes()).as_bytes());
        let manifest = Manifest {
            manifest_version: 1,
            transfer_id: "tx-A29".into(),
            source_device_id: URL_SAFE_NO_PAD.encode(blake3::hash(&[2u8; 32]).as_bytes()),
            total_tracks: 1,
            total_bytes: 6,
            items: vec![FileItem {
                logical_track_id: "local".into(),
                remote_track_id: "remote".into(),
                title: "title".into(),
                artist: String::new(),
                album: String::new(),
                format: "flac".into(),
                size: 6,
                sha256: "a".repeat(64),
            }],
        };
        let request = Request {
            intent: Intent {
                transfer_id: "tx-A29".into(),
                source_michi_id: URL_SAFE_NO_PAD.encode(blake3::hash(&[2u8; 32]).as_bytes()),
                destination_michi_id: URL_SAFE_NO_PAD.encode(blake3::hash(&[3u8; 32]).as_bytes()),
                source_library_id: "library".into(),
                destination_library_id: "dest-library".into(),
                selection_scope: "ALBUMS".into(),
                manifest_version: 1,
                required_scopes: default_scopes(),
                expires_at: 2_000_000,
                signed_grant: None,
                authorization_nonce_base64_url: String::new(),
            },
            manifest_sha256: manifest_sha256(&manifest).unwrap(),
            manifest,
            client_nonce_base64_url: URL_SAFE_NO_PAD.encode([7u8; 24]),
        };
        let ctx = AuthorizedContext {
            home_id,
            source_michi_id: URL_SAFE_NO_PAD.encode(blake3::hash(&[2u8; 32]).as_bytes()),
            destination_michi_id: URL_SAFE_NO_PAD.encode(blake3::hash(&[3u8; 32]).as_bytes()),
            source_library_id: "library".into(),
            allowed_destination_library_id: "dest-library".into(),
            is_active_home_member: true,
            may_read_original_files: true,
            approved_assets: vec![ApprovedAsset {
                remote_track_id: "remote".into(),
                exact_size: 6,
                exact_sha256: "a".repeat(64),
            }],
        };
        let root = michi_identity::HomeRootAuthority::from_signing_key(signer.clone());
        let grant = issue_grant(&request, &ctx, &root, 1_000_000).unwrap();
        (request, ctx, signer, grant)
    }
    #[test]
    fn selected_asset_guard_accepts_only_exact_original_asset() {
        let (req, ctx, key, grant) = selected_asset_fixture();
        assert!(authorize_original_asset_request(
            &grant,
            &key.verifying_key(),
            &ctx.destination_michi_id,
            &ctx,
            &req.manifest,
            "remote",
            6,
            &"a".repeat(64),
            1_000_002,
            false
        )
        .is_ok());
        assert_eq!(
            authorize_original_asset_request(
                &grant,
                &key.verifying_key(),
                &ctx.destination_michi_id,
                &ctx,
                &req.manifest,
                "different",
                6,
                &"a".repeat(64),
                1_000_002,
                false
            ),
            Err(Refusal::NotOriginalBytes)
        );
        assert_eq!(
            authorize_original_asset_request(
                &grant,
                &key.verifying_key(),
                &ctx.destination_michi_id,
                &ctx,
                &req.manifest,
                "remote",
                6,
                &"b".repeat(64),
                1_000_002,
                false
            ),
            Err(Refusal::NotOriginalBytes)
        );
    }
    #[test]
    fn selected_asset_guard_rejects_wrong_member_mutation_and_revocation() {
        let (req, ctx, key, grant) = selected_asset_fixture();
        assert_eq!(
            authorize_original_asset_request(
                &grant,
                &key.verifying_key(),
                "foreign",
                &ctx,
                &req.manifest,
                "remote",
                6,
                &"a".repeat(64),
                1_000_002,
                false
            ),
            Err(Refusal::WrongAudience)
        );
        let mut mutated = req.manifest.clone();
        mutated.items[0].title = "hijacked".into();
        assert_eq!(
            authorize_original_asset_request(
                &grant,
                &key.verifying_key(),
                &ctx.destination_michi_id,
                &ctx,
                &mutated,
                "remote",
                6,
                &"a".repeat(64),
                1_000_002,
                false
            ),
            Err(Refusal::WrongAudience)
        );
        assert_eq!(
            authorize_original_asset_request(
                &grant,
                &key.verifying_key(),
                &ctx.destination_michi_id,
                &ctx,
                &req.manifest,
                "remote",
                6,
                &"a".repeat(64),
                1_000_002,
                true
            ),
            Err(Refusal::Revoked)
        );
    }
    #[test]
    fn grant_with_delegated_scope_is_rejected_even_if_signed() {
        let (_, _, key, mut grant) = selected_asset_fixture();
        grant.scopes.push("content:delete".into());
        grant.signature_base64_url =
            URL_SAFE_NO_PAD.encode(key.sign(&grant_bytes(&grant).unwrap()).to_bytes());
        assert_eq!(
            verify_grant(
                &grant,
                &key.verifying_key(),
                &URL_SAFE_NO_PAD.encode(blake3::hash(&[3u8; 32]).as_bytes()),
                1_000_002,
                false
            ),
            Err(Refusal::InvalidFields)
        );
    }
}
