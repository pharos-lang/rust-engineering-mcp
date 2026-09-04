//! Identity and evidence of derived retrieval, never authoritative catalog facts.
use crate::{CatalogFingerprint, CatalogPage, Provenance, SnapshotEvidence};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticError {
    MissingModel,
    MissingIndex,
    InvalidArtifact,
    InvalidIndex,
    IdentityMismatch,
    InvalidInput,
    Budget,
    Inference,
}
impl std::fmt::Display for SemanticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "semantic operation unavailable: {self:?}")
    }
}
impl std::error::Error for SemanticError {}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingIdentity {
    pub model: String,
    pub revision: String,
    pub artifact_fingerprint: CatalogFingerprint,
    pub runtime: String,
    pub provenance: Provenance,
    pub dimension: u32,
    pub max_tokens: u32,
    pub intra_threads: u16,
    pub pooling: PoolingKind,
    pub normalization: Normalization,
}
impl EmbeddingIdentity {
    /// Bounds for internal construction. Persisted untrusted metadata has no
    /// Deserialize surface here; M1 import must introduce its own validated format.
    pub fn validate(&self) -> Result<(), SemanticError> {
        let valid_text = |s: &str, maximum: usize| {
            !s.trim().is_empty() && s.len() <= maximum && !s.contains('\0')
        };
        if !valid_text(&self.model, 128)
            || !valid_text(&self.revision, 128)
            || !valid_text(&self.runtime, 4096)
            || self.provenance.source_id().as_str().len() > 256
            || self.provenance.source_kind() != crate::SourceKind::EmbeddingModel
            || !(1..=1024).contains(&self.dimension)
            || !(1..=4096).contains(&self.max_tokens)
            || !(1..=64).contains(&self.intra_threads)
        {
            return Err(SemanticError::InvalidIndex);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolingKind {
    Mean,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Normalization {
    L2,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IndexMetadata {
    pub schema_version: u32,
    pub snapshot_fingerprint: CatalogFingerprint,
    pub model: EmbeddingIdentity,
}
#[derive(Clone, Debug)]
pub struct SemanticCandidate {
    pub crate_name: String,
    pub distance: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    Lexical,
    Hybrid,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HybridSearch {
    pub effective_mode: SearchMode,
    pub fallback: Option<SemanticError>,
    pub results: CatalogPage,
    pub semantic_index: Option<IndexMetadata>,
    pub model_evidence: Option<SnapshotEvidence>,
}

pub fn validate_embedding(vector: &[f32], dimension: u32) -> Result<(), SemanticError> {
    if dimension == 0
        || dimension > 1024
        || vector.len() != dimension as usize
        || vector.iter().any(|x| !x.is_finite())
    {
        return Err(SemanticError::InvalidIndex);
    }
    let norm = vector
        .iter()
        .map(|x| f64::from(*x).powi(2))
        .sum::<f64>()
        .sqrt();
    if (norm - 1.0).abs() > 0.001 {
        return Err(SemanticError::InvalidIndex);
    }
    Ok(())
}
