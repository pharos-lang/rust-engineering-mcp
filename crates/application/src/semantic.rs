//! Ports are effect boundaries. The future MCP adapter runs CPU inference on a bounded worker.
use crate::{CatalogRepository, search_catalog};
use rust_engineering_domain::*;
use std::{collections::HashSet, future::Future, pin::Pin};

pub trait EmbeddingProvider {
    fn identity(&self) -> &EmbeddingIdentity;
    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>, SemanticError>;
    fn embed_passage(&mut self, text: &str) -> Result<Vec<f32>, SemanticError>;
}
pub trait SemanticIndex {
    fn metadata(&self) -> &IndexMetadata;
    fn candidates<'a>(
        &'a self,
        query: &'a [f32],
        limit: u32,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SemanticCandidate>, SemanticError>> + Send + 'a>>;
}

/// Hybrid retrieval adds candidates; only SQLite may rehydrate facts. Every failure
/// preserves the usable lexical page and records why the semantic path was omitted.
pub async fn search_hybrid(
    repository: &impl CatalogRepository,
    query: &CatalogQuery,
    provider: Option<&mut dyn EmbeddingProvider>,
    index: Option<&dyn SemanticIndex>,
    policy: FreshnessPolicy,
    clock: &impl Clock,
) -> Result<HybridSearch, CatalogError> {
    let lexical = search_catalog(repository, query, policy.clone(), clock)?;
    let fallback = |reason| HybridSearch {
        effective_mode: SearchMode::Lexical,
        fallback: Some(reason),
        results: lexical.clone(),
        semantic_index: None,
        model_evidence: None,
    };
    let Some(provider) = provider else {
        return Ok(fallback(SemanticError::MissingModel));
    };
    let Some(index) = index else {
        return Ok(fallback(SemanticError::MissingIndex));
    };
    if index.metadata().model.validate().is_err()
        || index.metadata().schema_version != 1
        || index.metadata().snapshot_fingerprint != repository.metadata().fingerprint
        || &index.metadata().model != provider.identity()
    {
        return Ok(fallback(SemanticError::IdentityMismatch));
    }
    let vector = match provider.embed_query(query.text()) {
        Ok(v) => v,
        Err(e) => return Ok(fallback(e)),
    };
    if validate_embedding(&vector, provider.identity().dimension).is_err() {
        return Ok(fallback(SemanticError::InvalidIndex));
    }
    let candidates = match index.candidates(&vector, query.limit()).await {
        Ok(v) => v,
        Err(e) => return Ok(fallback(e)),
    };
    if candidates.len() > query.limit() as usize {
        return Ok(fallback(SemanticError::InvalidIndex));
    }
    let mut merged = lexical.crates.clone();
    let mut seen: HashSet<String> = merged.iter().map(|c| c.name.clone()).collect();
    let mut index_seen = HashSet::new();
    for candidate in candidates {
        if candidate.crate_name.is_empty()
            || candidate.crate_name.len() > 64
            || !candidate
                .crate_name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            || !candidate.distance.is_finite()
            || candidate.distance < 0.0
            || !index_seen.insert(candidate.crate_name.clone())
        {
            return Ok(fallback(SemanticError::InvalidIndex));
        }
        let fact = match repository.summary(&candidate.crate_name) {
            Ok(Some(v)) => v,
            Ok(None) => return Ok(fallback(SemanticError::InvalidIndex)),
            Err(error) => return Err(error),
        };
        if seen.insert(fact.name.clone()) && merged.len() < query.limit() as usize {
            merged.push(fact);
        }
    }
    // Conservative bound independent of any JSON library in application. Each UTF-8
    // byte needs at most six JSON bytes; fixed fields have generous reserved overhead.
    let bound: usize = merged
        .iter()
        .map(|c| {
            512 + 6
                * (c.name.len()
                    + c.description.len()
                    + c.latest_known.version.len()
                    + c.latest_known.rust_version.as_ref().map_or(0, String::len)
                    + c.latest_known.license.as_ref().map_or(0, String::len))
        })
        .sum();
    if bound > 128 * 1024 {
        return Ok(fallback(SemanticError::Budget));
    }
    Ok(HybridSearch {
        effective_mode: SearchMode::Hybrid,
        fallback: None,
        semantic_index: Some(index.metadata().clone()),
        model_evidence: Some(SnapshotEvidence::assess(
            provider.identity().provenance.clone(),
            policy,
            clock,
        )),
        results: CatalogPage {
            crates: merged,
            ..lexical
        },
    })
}
