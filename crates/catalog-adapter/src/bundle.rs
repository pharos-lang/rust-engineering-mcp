//! Authenticated owned-byte transport. Archive names never become filesystem paths.
mod floor;
use crate::{SnapshotManifest, SqliteCatalogRepository};
pub use floor::{FloorError, SequenceFloor};
use ring::signature::{ED25519, UnparsedPublicKey};
use rust_engineering_application::CatalogRepository;
use rust_engineering_domain::{CatalogError, CatalogFingerprint, IntegrityStatus, Provenance};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::time::{Duration, Instant};

pub const MAX_BUNDLE_BYTES: usize = 80 * 1024 * 1024;
pub const MAX_MANIFEST_BYTES: usize = 16 * 1024;
const MAX_FILES: usize = 16;
const SIGNING_CONTEXT: &[u8] = b"rust-engineering-catalog-bundle-v1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BundleError {
    InvalidTrust,
    UntrustedPublisher,
    InvalidSignature,
    InvalidArchive,
    NoncanonicalManifest,
    UnsupportedFormat,
    Integrity,
    Budget,
    Rollback,
    InvalidCatalog,
}
impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "catalog bundle rejected: {self:?}")
    }
}
impl std::error::Error for BundleError {}

/// Explicit host authority. A key contained in a bundle never authorizes itself.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublisherTrust {
    pub publisher: String,
    pub channel: String,
    /// Raw Ed25519 public key, 64 lowercase hexadecimal characters.
    pub public_key: String,
}
impl PublisherTrust {
    pub fn parse(bytes: &[u8]) -> Result<Self, BundleError> {
        if bytes.len() > 4096 {
            return Err(BundleError::Budget);
        }
        let trust: Self = serde_json::from_slice(bytes).map_err(|_| BundleError::InvalidTrust)?;
        trust.key()?;
        Ok(trust)
    }
    fn key(&self) -> Result<[u8; 32], BundleError> {
        if !identifier(&self.publisher) || !identifier(&self.channel) || self.public_key.len() != 64
        {
            return Err(BundleError::InvalidTrust);
        }
        let mut result = [0; 32];
        for (out, pair) in result
            .iter_mut()
            .zip(self.public_key.as_bytes().as_chunks::<2>().0)
        {
            let digit = |b| match b {
                b'0'..=b'9' => Ok(b - b'0'),
                b'a'..=b'f' => Ok(b - b'a' + 10),
                _ => Err(BundleError::InvalidTrust),
            };
            *out = digit(pair[0])? * 16 + digit(pair[1])?;
        }
        Ok(result)
    }
    pub fn key_fingerprint(&self) -> Result<String, BundleError> {
        Ok(sha256(&self.key()?))
    }
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleFile {
    pub path: String,
    pub byte_length: u64,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleManifest {
    pub snapshot_format_version: u32,
    pub catalog_schema_version: u32,
    pub semantic_index_version: Option<u32>,
    pub embedding_model_id: Option<String>,
    pub publisher: String,
    pub channel: String,
    pub sequence: u64,
    /// Preserved source observation; import time never refreshes these timestamps.
    pub catalog_provenance: Provenance,
    pub files: Vec<BundleFile>,
}

pub struct VerifiedBundle {
    manifest: BundleManifest,
    repository: SqliteCatalogRepository,
    rustsec: Option<Vec<u8>>,
    semantic_index: Option<Vec<u8>>,
    fingerprint: String,
}
impl VerifiedBundle {
    pub fn manifest(&self) -> &BundleManifest {
        &self.manifest
    }
    pub fn repository(&self) -> &SqliteCatalogRepository {
        &self.repository
    }
    pub fn rustsec_bytes(&self) -> Option<&[u8]> {
        self.rustsec.as_deref()
    }
    pub fn semantic_index_bytes(&self) -> Option<&[u8]> {
        self.semantic_index.as_deref()
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub fn require_newer_than(&self, sequence: u64) -> Result<(), BundleError> {
        if self.manifest.sequence <= sequence {
            Err(BundleError::Rollback)
        } else {
            Ok(())
        }
    }
}

/// Authenticates each generation, including an active record reread after restart.
/// Signature is domain-separated from every other use of the publisher's key.
pub fn verify(bytes: &[u8], trust: &PublisherTrust) -> Result<VerifiedBundle, BundleError> {
    let key = trust.key()?;
    if bytes.len() > MAX_BUNDLE_BYTES {
        return Err(BundleError::Budget);
    }
    let started = Instant::now();
    let mut decoder =
        zstd::stream::read::Decoder::with_buffer(bytes).map_err(|_| BundleError::InvalidArchive)?;
    decoder
        .window_log_max(23)
        .map_err(|_| BundleError::InvalidArchive)?;
    let mut archive = Vec::new();
    let mut chunk = [0; 64 * 1024];
    loop {
        if started.elapsed() > Duration::from_secs(30) {
            return Err(BundleError::Budget);
        }
        let allowed = chunk.len().min(MAX_BUNDLE_BYTES - archive.len() + 1);
        let n = decoder
            .read(&mut chunk[..allowed])
            .map_err(|_| BundleError::InvalidArchive)?;
        if n == 0 {
            break;
        }
        if n > MAX_BUNDLE_BYTES - archive.len() {
            return Err(BundleError::Budget);
        }
        archive.try_reserve(n).map_err(|_| BundleError::Budget)?;
        archive.extend_from_slice(&chunk[..n]);
    }
    let entries = archive::entries(&archive)?;
    if entries.len() < 3 || entries[0].0 != "manifest.json" || entries[1].0 != "signature.ed25519" {
        return Err(BundleError::InvalidArchive);
    }
    let manifest_bytes = entries[0].1;
    if manifest_bytes.len() > MAX_MANIFEST_BYTES {
        return Err(BundleError::Budget);
    }
    let signature = entries[1].1;
    if signature.len() != 64 {
        return Err(BundleError::InvalidSignature);
    }
    let mut signed = SIGNING_CONTEXT.to_vec();
    signed.extend_from_slice(manifest_bytes);
    UnparsedPublicKey::new(&ED25519, key)
        .verify(&signed, signature)
        .map_err(|_| BundleError::InvalidSignature)?;
    let manifest: BundleManifest =
        serde_json::from_slice(manifest_bytes).map_err(|_| BundleError::NoncanonicalManifest)?;
    if serde_json::to_vec(&manifest).map_err(|_| BundleError::NoncanonicalManifest)?
        != manifest_bytes
    {
        return Err(BundleError::NoncanonicalManifest);
    }
    if manifest.publisher != trust.publisher || manifest.channel != trust.channel {
        return Err(BundleError::UntrustedPublisher);
    }
    if manifest.snapshot_format_version != 1 || manifest.catalog_schema_version != 1 {
        return Err(BundleError::UnsupportedFormat);
    }
    if manifest.sequence == 0
        || manifest.sequence > i64::MAX as u64
        || manifest.files.is_empty()
        || manifest.files.len() > MAX_FILES - 2
    {
        return Err(BundleError::Budget);
    }
    if manifest.files.len() != entries.len() - 2 {
        return Err(BundleError::Integrity);
    }
    let mut previous = "";
    for (file, (path, data)) in manifest.files.iter().zip(&entries[2..]) {
        if file.path.as_str() <= previous
            || file.path != *path
            || file.byte_length != data.len() as u64
            || file.sha256 != sha256(data)
        {
            return Err(BundleError::Integrity);
        }
        if !matches!(
            path.as_str(),
            "catalog.sqlite" | "rustsec.json" | "semantic.index"
        ) {
            return Err(BundleError::UnsupportedFormat);
        }
        previous = &file.path;
    }
    if entries[2].0 != "catalog.sqlite" {
        return Err(BundleError::InvalidCatalog);
    }
    let expected = SnapshotManifest {
        format_version: 1,
        sequence: manifest.sequence,
        byte_length: entries[2].1.len() as u64,
        fingerprint: format!("sha256:{}", manifest.files[0].sha256)
            .parse::<CatalogFingerprint>()
            .map_err(|_| BundleError::Integrity)?,
    };
    let repository = SqliteCatalogRepository::open(entries[2].1, &expected).map_err(map_catalog)?;
    if repository.metadata().provenance != manifest.catalog_provenance
        || manifest.catalog_provenance.integrity() != IntegrityStatus::Verified
    {
        return Err(BundleError::Integrity);
    }
    let rustsec = if let Some((_, bytes)) = entries.iter().find(|(name, _)| name == "rustsec.json")
    {
        let fingerprint = format!("sha256:{}", sha256(bytes))
            .parse()
            .map_err(|_| BundleError::Integrity)?;
        crate::RustSecSnapshot::from_bytes(bytes, &fingerprint, &Deadline(started))
            .map_err(|_| BundleError::InvalidCatalog)?;
        Some(bytes.to_vec())
    } else {
        None
    };
    let semantic_index = entries
        .iter()
        .find(|(name, _)| name == "semantic.index")
        .map(|(_, bytes)| bytes.to_vec());
    match (
        &semantic_index,
        manifest.semantic_index_version,
        manifest.embedding_model_id.as_deref(),
    ) {
        (None, None, None) => {}
        (Some(bytes), Some(1), Some("intfloat/multilingual-e5-small"))
            if bytes.len() <= 16 * 1024 * 1024 => {}
        _ => return Err(BundleError::UnsupportedFormat),
    }
    Ok(VerifiedBundle {
        manifest,
        repository,
        rustsec,
        semantic_index,
        fingerprint: sha256(bytes),
    })
}
struct Deadline(Instant);
impl rust_engineering_application::OperationControl for Deadline {
    fn check(&self) -> Result<(), rust_engineering_application::ProjectError> {
        if self.0.elapsed() > Duration::from_secs(60) {
            Err(rust_engineering_application::ProjectError::Cancelled)
        } else {
            Ok(())
        }
    }
}
impl rust_engineering_application::ExecutionCancellation for Deadline {
    fn is_cancelled(&self) -> bool {
        self.0.elapsed() > Duration::from_secs(60)
    }
}
fn map_catalog(error: CatalogError) -> BundleError {
    match error {
        CatalogError::Budget => BundleError::Budget,
        CatalogError::UnsupportedSchema => BundleError::UnsupportedFormat,
        _ => BundleError::InvalidCatalog,
    }
}
pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
mod archive;
#[cfg(test)]
mod tests;
