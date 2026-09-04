//! Explicit host-owned model/index bytes. No network or caller-selected Lance URI.
#[cfg(feature = "local")]
use rust_engineering_application::{CatalogRepository, EmbeddingProvider};
#[cfg(feature = "local")]
use rust_engineering_domain::IndexMetadata;
use rust_engineering_domain::SemanticError;
use std::path::Path;

#[cfg(feature = "local")]
pub fn load_model(
    path: &Path,
) -> Result<rust_engineering_semantic::LocalEmbeddingProvider, SemanticError> {
    use rust_engineering_semantic::{
        E5_FILES, LocalEmbeddingProvider, OfflineRuntime, VerifiedE5Bundle,
    };
    let mut files: [Vec<u8>; E5_FILES.len()] = Default::default();
    for (output, (name, size, _)) in files.iter_mut().zip(E5_FILES) {
        *output = rust_engineering_project::catalog_store::read_model_file(&path.join(name), size)
            .map_err(|_| SemanticError::InvalidArtifact)?;
    }
    let model = VerifiedE5Bundle::verify(files)?;
    let runtime = OfflineRuntime::initialize()?;
    LocalEmbeddingProvider::load(&runtime, model)
}
#[cfg(feature = "local")]
pub fn rebuild(
    repository: &rust_engineering_catalog::SqliteCatalogRepository,
    model_path: &Path,
) -> Result<Vec<u8>, SemanticError> {
    let started = std::time::Instant::now();
    let mut provider = load_model(model_path)?;
    rebuild_budget(started)?;
    let documents = repository
        .embedding_documents()
        .map_err(|_| SemanticError::InvalidIndex)?;
    // SQLite already caps this count; retain a local bound before native work.
    if documents.len() > 1000 {
        return Err(SemanticError::Budget);
    }
    let mut rows = Vec::with_capacity(documents.len());
    for (name, text) in documents {
        rebuild_budget(started)?;
        rows.push((name, provider.embed_passage(&text)?));
        rebuild_budget(started)?;
    }
    let metadata = IndexMetadata {
        schema_version: 1,
        snapshot_fingerprint: repository.metadata().fingerprint.clone(),
        model: provider.identity().clone(),
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| SemanticError::Inference)?
        .block_on(async {
            let index =
                rust_engineering_semantic::LanceMemoryIndex::build(metadata.clone(), rows).await?;
            rebuild_budget(started)?;
            let bytes = index.export().await?;
            rebuild_budget(started)?;
            drop(index);
            // Reopen the exact native objects, never reconstruct them from row fixtures.
            rust_engineering_semantic::LanceMemoryIndex::restore(&bytes, metadata).await?;
            rebuild_budget(started)?;
            Ok(bytes)
        })
}
#[cfg(not(feature = "local"))]
pub fn rebuild(
    _: &rust_engineering_catalog::SqliteCatalogRepository,
    _: &Path,
) -> Result<Vec<u8>, SemanticError> {
    Err(SemanticError::MissingModel)
}

#[cfg(feature = "local")]
pub fn validate_imported_index(
    bundle: &rust_engineering_catalog::bundle::VerifiedBundle,
    model_path: Option<&Path>,
) -> Result<(), SemanticError> {
    let Some(bytes) = bundle.semantic_index_bytes() else {
        return Ok(());
    };
    let provider = load_model(model_path.ok_or(SemanticError::MissingModel)?)?;
    let metadata = IndexMetadata {
        schema_version: 1,
        snapshot_fingerprint: bundle.repository().metadata().fingerprint.clone(),
        model: provider.identity().clone(),
    };
    let index = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| SemanticError::Inference)?
        .block_on(rust_engineering_semantic::LanceMemoryIndex::restore(
            bytes, metadata,
        ))?;
    let mut names: Vec<_> = bundle
        .repository()
        .embedding_documents()
        .map_err(|_| SemanticError::InvalidIndex)?
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    names.sort();
    if names != index.crate_names() {
        return Err(SemanticError::InvalidIndex);
    }
    Ok(())
}
#[cfg(not(feature = "local"))]
pub fn validate_imported_index(
    bundle: &rust_engineering_catalog::bundle::VerifiedBundle,
    _: Option<&Path>,
) -> Result<(), SemanticError> {
    if bundle.semantic_index_bytes().is_some() {
        Err(SemanticError::MissingModel)
    } else {
        Ok(())
    }
}

#[cfg(feature = "local")]
pub fn validate_persisted_index(
    repository: &rust_engineering_catalog::SqliteCatalogRepository,
    model_path: &Path,
    index_path: &Path,
) -> Result<(), SemanticError> {
    let bytes = rust_engineering_project::catalog_store::read_trust_file(
        &index_path.join("active.bundle"),
        16 * 1024 * 1024,
    )
    .map_err(|_| SemanticError::MissingIndex)?;
    let provider = load_model(model_path)?;
    let expected = IndexMetadata {
        schema_version: 1,
        snapshot_fingerprint: repository.metadata().fingerprint.clone(),
        model: provider.identity().clone(),
    };
    let index = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| SemanticError::Inference)?
        .block_on(rust_engineering_semantic::LanceMemoryIndex::restore(
            &bytes, expected,
        ))?;
    let mut names: Vec<_> = repository
        .embedding_documents()
        .map_err(|_| SemanticError::InvalidIndex)?
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    names.sort();
    if index.crate_names() != names {
        return Err(SemanticError::InvalidIndex);
    }
    Ok(())
}
#[cfg(not(feature = "local"))]
pub fn validate_persisted_index(
    _: &rust_engineering_catalog::SqliteCatalogRepository,
    _: &Path,
    _: &Path,
) -> Result<(), SemanticError> {
    Err(SemanticError::MissingModel)
}

#[cfg(feature = "local")]
fn rebuild_budget(started: std::time::Instant) -> Result<(), SemanticError> {
    if started.elapsed() > std::time::Duration::from_secs(300) {
        Err(SemanticError::Budget)
    } else {
        Ok(())
    }
}
