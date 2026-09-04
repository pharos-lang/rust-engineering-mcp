//! Bounded, derived LanceDB generations held entirely by their memory-store handles.
use std::{collections::HashSet, future::Future, pin::Pin, sync::Arc};

use arrow_array::{
    Array, FixedSizeListArray, Float32Array, RecordBatch, StringArray, types::Float32Type,
};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::{
    Connection, DistanceType, Table,
    query::{ExecutableQuery, QueryBase, Select},
};
use rust_engineering_application::SemanticIndex;
use rust_engineering_domain::{
    IndexMetadata, SemanticCandidate, SemanticError, validate_embedding,
};

const MAX_ROWS: usize = 1000;
const MAX_LIMIT: u32 = 50;
mod persistence;

/// A complete generation: metadata and vectors are replaced together on rebuild.
pub struct LanceMemoryIndex {
    metadata: IndexMetadata,
    table: Table,
    // Retain the connection's memory store for the generation's entire lifetime.
    _connection: Connection,
    session: Arc<lancedb::Session>,
    crate_names: HashSet<String>,
}

impl LanceMemoryIndex {
    /// Validated native-table identifiers; facts are still resolved by SQLite.
    pub fn crate_names(&self) -> Vec<String> {
        let mut names: Vec<_> = self.crate_names.iter().cloned().collect();
        names.sort();
        names
    }
    /// Build from owned, bounded embeddings. No caller-selected storage URI exists.
    pub async fn build(
        metadata: IndexMetadata,
        rows: Vec<(String, Vec<f32>)>,
    ) -> Result<Self, SemanticError> {
        if metadata.schema_version != 1 || metadata.model.validate().is_err() {
            return Err(SemanticError::InvalidIndex);
        }
        if rows.len() > MAX_ROWS {
            return Err(SemanticError::Budget);
        }
        let mut crate_names = HashSet::with_capacity(rows.len());
        for (name, vector) in &rows {
            if !valid_crate_name(name) || !crate_names.insert(name.clone()) {
                return Err(SemanticError::InvalidIndex);
            }
            validate_embedding(vector, metadata.model.dimension)?;
        }
        let dimension =
            i32::try_from(metadata.model.dimension).map_err(|_| SemanticError::InvalidIndex)?;
        let schema = index_schema(dimension);
        let names = StringArray::from_iter_values(rows.iter().map(|(name, _)| name.as_str()));
        let vectors = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
            rows.into_iter()
                .map(|(_, vector)| Some(vector.into_iter().map(Some))),
            dimension,
        );
        let batch = RecordBatch::try_new(schema, vec![Arc::new(names), Arc::new(vectors)])
            .map_err(|_| SemanticError::InvalidIndex)?;
        // A new connection owns a fresh InMemory object store. The URI is fixed,
        // and is never shared with another generation via shared-memory storage.
        let session = persistence::memory_session()?;
        let connection = lancedb::connect("memory://")
            .session(session.clone())
            .execute()
            .await
            .map_err(|_| SemanticError::InvalidIndex)?;
        let table = connection
            .create_table("crate_vectors", batch)
            .execute()
            .await
            .map_err(|_| SemanticError::InvalidIndex)?;
        if table
            .count_rows(None)
            .await
            .map_err(|_| SemanticError::InvalidIndex)?
            != crate_names.len()
        {
            return Err(SemanticError::InvalidIndex);
        }
        Ok(Self {
            metadata,
            table,
            _connection: connection,
            session,
            crate_names,
        })
    }

    /// Failure or cancellation before assignment leaves the current generation intact.
    pub async fn rebuild(
        &mut self,
        metadata: IndexMetadata,
        rows: Vec<(String, Vec<f32>)>,
    ) -> Result<(), SemanticError> {
        let candidate = Self::build(metadata, rows).await?;
        *self = candidate;
        Ok(())
    }

    async fn search(
        &self,
        query: &[f32],
        limit: u32,
    ) -> Result<Vec<SemanticCandidate>, SemanticError> {
        if !(1..=MAX_LIMIT).contains(&limit) {
            return Err(SemanticError::InvalidInput);
        }
        validate_embedding(query, self.metadata.model.dimension)
            .map_err(|_| SemanticError::InvalidInput)?;
        let expected = self.crate_names.len().min(limit as usize);
        if expected == 0 {
            return Ok(Vec::new());
        }
        let mut batches = self
            .table
            .vector_search(query)
            .map_err(|_| SemanticError::InvalidIndex)?
            .distance_type(DistanceType::L2)
            .bypass_vector_index()
            .select(Select::columns(&["crate_name"]))
            .limit(limit as usize)
            .execute()
            .await
            .map_err(|_| SemanticError::InvalidIndex)?;
        let mut candidates = Vec::with_capacity(expected);
        let mut seen = HashSet::with_capacity(expected);
        while let Some(batch) = batches
            .try_next()
            .await
            .map_err(|_| SemanticError::InvalidIndex)?
        {
            if batch.num_rows() > expected.saturating_sub(candidates.len()) {
                return Err(SemanticError::InvalidIndex);
            }
            let names = batch
                .column_by_name("crate_name")
                .and_then(|array| array.as_any().downcast_ref::<StringArray>())
                .ok_or(SemanticError::InvalidIndex)?;
            let distances = batch
                .column_by_name("_distance")
                .and_then(|array| array.as_any().downcast_ref::<Float32Array>())
                .ok_or(SemanticError::InvalidIndex)?;
            for row in 0..batch.num_rows() {
                if names.is_null(row) || distances.is_null(row) {
                    return Err(SemanticError::InvalidIndex);
                }
                let name = names.value(row);
                let distance = distances.value(row);
                if !valid_crate_name(name)
                    || !self.crate_names.contains(name)
                    || !seen.insert(name.to_owned())
                    || !distance.is_finite()
                    || distance < 0.0
                    || candidates
                        .last()
                        .is_some_and(|previous: &SemanticCandidate| previous.distance > distance)
                {
                    return Err(SemanticError::InvalidIndex);
                }
                candidates.push(SemanticCandidate {
                    crate_name: name.to_owned(),
                    distance,
                });
            }
        }
        if candidates.len() != expected {
            return Err(SemanticError::InvalidIndex);
        }
        Ok(candidates)
    }
}

impl SemanticIndex for LanceMemoryIndex {
    fn metadata(&self) -> &IndexMetadata {
        &self.metadata
    }

    fn candidates<'a>(
        &'a self,
        query: &'a [f32],
        limit: u32,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SemanticCandidate>, SemanticError>> + Send + 'a>>
    {
        Box::pin(self.search(query, limit))
    }
}

fn valid_crate_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn index_schema(dimension: i32) -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("crate_name", DataType::Utf8, false),
        Field::new(
            "vector",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                dimension,
            ),
            false,
        ),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_domain::{
        EmbeddingIdentity, IntegrityStatus, Normalization, PoolingKind, Provenance, SourceKind,
        UnixSeconds,
    };

    pub(super) type TestResult = Result<(), Box<dyn std::error::Error>>;

    pub(super) fn metadata(
        generation: char,
        dimension: u32,
    ) -> Result<IndexMetadata, Box<dyn std::error::Error>> {
        Ok(IndexMetadata {
            schema_version: 1,
            snapshot_fingerprint: format!("sha256:{}", generation.to_string().repeat(64))
                .parse()?,
            model: EmbeddingIdentity {
                model: "index-test-vectors".to_owned(),
                revision: "fixture-1".to_owned(),
                artifact_fingerprint: format!("sha256:{}", "a".repeat(64)).parse()?,
                runtime: "fixture".to_owned(),
                provenance: Provenance::new(
                    SourceKind::EmbeddingModel,
                    "index-test-vectors".parse()?,
                    Some(UnixSeconds(1)),
                    Some(UnixSeconds(1)),
                    IntegrityStatus::Verified,
                    false,
                )?,
                dimension,
                max_tokens: 512,
                intra_threads: 2,
                pooling: PoolingKind::Mean,
                normalization: Normalization::L2,
            },
        })
    }

    pub(super) fn rows() -> Vec<(String, Vec<f32>)> {
        vec![
            ("serde".to_owned(), vec![1.0, 0.0]),
            ("tokio".to_owned(), vec![0.0, 1.0]),
            ("opposite".to_owned(), vec![-1.0, 0.0]),
        ]
    }

    #[tokio::test]
    async fn real_table_returns_only_bounded_ids_and_distances() -> TestResult {
        let index = LanceMemoryIndex::build(metadata('1', 2)?, rows()).await?;
        let candidates = index.candidates(&[1.0, 0.0], 2).await?;
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].crate_name, "serde");
        assert!(candidates[0].distance.abs() < 0.00001);
        assert_eq!(candidates[1].crate_name, "tokio");
        assert!((candidates[1].distance - 2.0).abs() < 0.00001);
        assert_eq!(index.candidates(&[1.0, 0.0], 50).await?.len(), 3);
        let schema = index.table.schema().await?;
        assert_eq!(
            schema.field_with_name("crate_name")?.data_type(),
            &DataType::Utf8
        );
        assert!(matches!(
            schema.field_with_name("vector")?.data_type(),
            DataType::FixedSizeList(_, 2)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn rejects_invalid_metadata_and_rows_before_generation() -> TestResult {
        for dimension in [0, 1025] {
            assert!(matches!(
                LanceMemoryIndex::build(metadata('1', dimension)?, vec![]).await,
                Err(SemanticError::InvalidIndex)
            ));
        }
        for field in 0..5 {
            let mut bad = metadata('1', 2)?;
            match field {
                0 => bad.model.max_tokens = 0,
                1 => bad.model.intra_threads = 0,
                2 => bad.model.runtime = "r".repeat(4097),
                3 => bad.model.model.clear(),
                _ => bad.model.revision = "r".repeat(129),
            }
            assert!(matches!(
                LanceMemoryIndex::build(bad, rows()).await,
                Err(SemanticError::InvalidIndex)
            ));
        }
        let mut wrong_schema = metadata('1', 2)?;
        wrong_schema.schema_version = 2;
        assert!(matches!(
            LanceMemoryIndex::build(wrong_schema, rows()).await,
            Err(SemanticError::InvalidIndex)
        ));
        for name in [
            "",
            "../serde",
            "serde/json",
            "serde.json",
            "sécurité",
            "a b",
            "a\0b",
        ] {
            assert!(matches!(
                LanceMemoryIndex::build(metadata('1', 2)?, vec![(name.to_owned(), vec![1.0, 0.0])])
                    .await,
                Err(SemanticError::InvalidIndex)
            ));
        }
        assert!(matches!(
            LanceMemoryIndex::build(metadata('1', 2)?, vec![("a".repeat(65), vec![1.0, 0.0])])
                .await,
            Err(SemanticError::InvalidIndex)
        ));
        let mut duplicates = rows();
        duplicates.push(("serde".to_owned(), vec![0.0, 1.0]));
        assert!(matches!(
            LanceMemoryIndex::build(metadata('1', 2)?, duplicates).await,
            Err(SemanticError::InvalidIndex)
        ));
        for vector in [
            vec![],
            vec![1.0],
            vec![1.0, 0.0, 0.0],
            vec![0.0, 0.0],
            vec![2.0, 0.0],
            vec![f32::NAN, 0.0],
            vec![f32::INFINITY, 0.0],
        ] {
            assert!(matches!(
                LanceMemoryIndex::build(metadata('1', 2)?, vec![("valid".to_owned(), vector)])
                    .await,
                Err(SemanticError::InvalidIndex)
            ));
        }
        assert!(matches!(
            LanceMemoryIndex::build(
                metadata('1', 2)?,
                vec![("valid".to_owned(), vec![1.0, 0.0]); MAX_ROWS + 1]
            )
            .await,
            Err(SemanticError::Budget)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn query_limits_and_vectors_are_validated_even_for_empty_table() -> TestResult {
        let index = LanceMemoryIndex::build(metadata('1', 2)?, vec![]).await?;
        assert!(index.candidates(&[1.0, 0.0], 1).await?.is_empty());
        for limit in [0, 51, u32::MAX] {
            assert!(matches!(
                index.candidates(&[1.0, 0.0], limit).await,
                Err(SemanticError::InvalidInput)
            ));
        }
        for vector in [
            vec![],
            vec![1.0],
            vec![0.0, 0.0],
            vec![f32::NAN, 0.0],
            vec![f32::NEG_INFINITY, 0.0],
        ] {
            assert!(matches!(
                index.candidates(&vector, 1).await,
                Err(SemanticError::InvalidInput)
            ));
        }
        Ok(())
    }

    #[tokio::test]
    async fn failed_rebuild_preserves_generation_and_success_replaces_it() -> TestResult {
        let original = metadata('1', 2)?;
        let mut index = LanceMemoryIndex::build(original.clone(), rows()).await?;
        assert_eq!(
            index
                .rebuild(metadata('2', 2)?, vec![("bad".to_owned(), vec![0.0, 0.0])])
                .await,
            Err(SemanticError::InvalidIndex)
        );
        assert_eq!(index.metadata(), &original);
        assert_eq!(
            index.candidates(&[1.0, 0.0], 1).await?[0].crate_name,
            "serde"
        );
        let next = metadata('2', 2)?;
        index
            .rebuild(
                next.clone(),
                vec![("replacement".to_owned(), vec![1.0, 0.0])],
            )
            .await?;
        assert_eq!(index.metadata(), &next);
        assert_eq!(
            index.candidates(&[1.0, 0.0], 50).await?[0].crate_name,
            "replacement"
        );
        Ok(())
    }

    #[tokio::test]
    async fn memory_generations_do_not_share_tables() -> TestResult {
        let first = LanceMemoryIndex::build(metadata('1', 2)?, rows()).await?;
        let second = LanceMemoryIndex::build(
            metadata('2', 2)?,
            vec![("Second_crate-2".to_owned(), vec![1.0, 0.0])],
        )
        .await?;
        assert_eq!(
            first.candidates(&[1.0, 0.0], 1).await?[0].crate_name,
            "serde"
        );
        assert_eq!(
            second.candidates(&[1.0, 0.0], 1).await?[0].crate_name,
            "Second_crate-2"
        );
        Ok(())
    }

    #[tokio::test]
    async fn accepts_maximum_rows_dimension_and_name_length() -> TestResult {
        let mut vector = vec![0.0; 1024];
        vector[0] = 1.0;
        let rows = (0..MAX_ROWS)
            .map(|i| (format!("{i:064}"), vector.clone()))
            .collect();
        let index = LanceMemoryIndex::build(metadata('1', 1024)?, rows).await?;
        assert_eq!(index.candidates(&vector, 50).await?.len(), 50);
        assert_eq!(index.crate_names.len(), MAX_ROWS);
        Ok(())
    }
}
