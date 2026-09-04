//! Native Lance 8 object snapshots. Object keys never become host filesystem paths.
use super::*;
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 16] = b"REMCP-LANCE8-V1\0";
const MAX_ARCHIVE: usize = 16 * 1024 * 1024;
const MAX_OBJECT: usize = 8 * 1024 * 1024;
const MAX_OBJECTS: usize = 128;
const MAX_METADATA: usize = 16 * 1024;
const MAX_PATH: usize = 256;

pub(super) fn memory_session() -> Result<Arc<lancedb::Session>, SemanticError> {
    let registry = Arc::new(lancedb::ObjectStoreRegistry::empty());
    let provider = lancedb::ObjectStoreRegistry::default()
        .get_provider("memory")
        .ok_or(SemanticError::InvalidIndex)?;
    registry.insert("memory", provider);
    Ok(Arc::new(lancedb::Session::new(
        8 * 1024 * 1024,
        8 * 1024 * 1024,
        registry,
    )))
}

struct Object<'a> {
    path: &'a str,
    bytes: &'a [u8],
}

fn canonical_path(path: &str) -> bool {
    path.len() <= MAX_PATH
        && path
            .strip_prefix("crate_vectors.lance/")
            .is_some_and(|tail| {
                !tail.is_empty()
                    && tail.split('/').all(|part| {
                        !part.is_empty()
                            && part != "."
                            && part != ".."
                            && part.bytes().all(|b| {
                                b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')
                            })
                    })
            })
}

fn metadata_bytes(metadata: &IndexMetadata) -> Result<Vec<u8>, SemanticError> {
    if metadata.schema_version != 1 {
        return Err(SemanticError::InvalidIndex);
    }
    metadata.model.validate()?;
    let bytes = serde_json::to_vec(metadata).map_err(|_| SemanticError::InvalidIndex)?;
    if bytes.len() > MAX_METADATA {
        return Err(SemanticError::Budget);
    }
    Ok(bytes)
}

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], SemanticError> {
        if count > self.0.len() {
            return Err(SemanticError::InvalidIndex);
        }
        let (head, tail) = self.0.split_at(count);
        self.0 = tail;
        Ok(head)
    }
    fn number(&mut self) -> Result<usize, SemanticError> {
        let bytes = self
            .take(4)?
            .try_into()
            .map_err(|_| SemanticError::InvalidIndex)?;
        usize::try_from(u32::from_le_bytes(bytes)).map_err(|_| SemanticError::Budget)
    }
}

fn number(output: &mut Vec<u8>, value: usize) -> Result<(), SemanticError> {
    output.extend_from_slice(
        &u32::try_from(value)
            .map_err(|_| SemanticError::Budget)?
            .to_le_bytes(),
    );
    Ok(())
}

// Verify every object before passing any bytes to native Lance parsers.
fn decode<'a>(bytes: &'a [u8], expected: &IndexMetadata) -> Result<Vec<Object<'a>>, SemanticError> {
    if bytes.len() > MAX_ARCHIVE {
        return Err(SemanticError::Budget);
    }
    let mut input = Reader(bytes);
    if input.take(MAGIC.len())? != MAGIC {
        return Err(SemanticError::InvalidIndex);
    }
    let size = input.number()?;
    if size > MAX_METADATA {
        return Err(SemanticError::Budget);
    }
    if input.take(size)? != metadata_bytes(expected)? {
        return Err(SemanticError::IdentityMismatch);
    }
    let count = input.number()?;
    if count == 0 {
        return Err(SemanticError::InvalidIndex);
    }
    if count > MAX_OBJECTS {
        return Err(SemanticError::Budget);
    }
    let mut objects = Vec::with_capacity(count);
    let mut previous = "";
    for _ in 0..count {
        let size = input.number()?;
        if size > MAX_PATH {
            return Err(SemanticError::Budget);
        }
        let path =
            std::str::from_utf8(input.take(size)?).map_err(|_| SemanticError::InvalidIndex)?;
        if !canonical_path(path) || path <= previous {
            return Err(SemanticError::InvalidIndex);
        }
        previous = path;
        let size = input.number()?;
        if size > MAX_OBJECT {
            return Err(SemanticError::Budget);
        }
        let hash = input.take(32)?;
        let payload = input.take(size)?;
        if Sha256::digest(payload).as_slice() != hash {
            return Err(SemanticError::InvalidIndex);
        }
        objects.push(Object {
            path,
            bytes: payload,
        });
    }
    if !input.0.is_empty() {
        return Err(SemanticError::InvalidIndex);
    }
    Ok(objects)
}

impl LanceMemoryIndex {
    /// Export immutable native objects, not embedding rows. Caller owns durable I/O.
    pub async fn export(&self) -> Result<Vec<u8>, SemanticError> {
        let registry = self.session.store_registry();
        let store = registry
            .get_store(
                "memory://"
                    .parse()
                    .map_err(|_| SemanticError::InvalidIndex)?,
                &Default::default(),
            )
            .await
            .map_err(|_| SemanticError::InvalidIndex)?;
        // LanceDB creates independent __manifest namespace bookkeeping on connect.
        // Persist only this generation's native table objects; fresh connections
        // regenerate their own bookkeeping without importing extra store authority.
        let prefix = "crate_vectors.lance".into();
        let mut listing = store.inner.list(Some(&prefix));
        let mut objects = Vec::new();
        let metadata = metadata_bytes(&self.metadata)?;
        let mut total = MAGIC.len() + 8 + metadata.len();
        while let Some(object) = listing
            .try_next()
            .await
            .map_err(|_| SemanticError::InvalidIndex)?
        {
            if objects.len() >= MAX_OBJECTS || object.size > MAX_OBJECT as u64 {
                return Err(SemanticError::Budget);
            }
            let path = object.location.as_ref();
            if !canonical_path(path) {
                return Err(SemanticError::InvalidIndex);
            }
            total = total
                .checked_add(path.len() + 40 + object.size as usize)
                .ok_or(SemanticError::Budget)?;
            if total > MAX_ARCHIVE {
                return Err(SemanticError::Budget);
            }
            objects.push(object);
        }
        if objects.is_empty() {
            return Err(SemanticError::InvalidIndex);
        }
        objects.sort_by(|a, b| a.location.cmp(&b.location));
        let mut output = Vec::with_capacity(total);
        output.extend_from_slice(MAGIC);
        number(&mut output, metadata.len())?;
        output.extend_from_slice(&metadata);
        number(&mut output, objects.len())?;
        for object in objects {
            let bytes = store
                .inner
                .get_opts(&object.location, Default::default())
                .await
                .map_err(|_| SemanticError::InvalidIndex)?
                .bytes()
                .await
                .map_err(|_| SemanticError::InvalidIndex)?;
            if bytes.len() as u64 != object.size {
                return Err(SemanticError::InvalidIndex);
            }
            let path = object.location.as_ref();
            number(&mut output, path.len())?;
            output.extend_from_slice(path.as_bytes());
            number(&mut output, bytes.len())?;
            output.extend_from_slice(&Sha256::digest(&bytes));
            output.extend_from_slice(&bytes);
        }
        Ok(output)
    }

    /// Restore native objects only after exact identity, bounds and hash verification.
    /// Authentication/acquisition belong to the caller's trusted artifact boundary.
    pub async fn restore(bytes: &[u8], expected: IndexMetadata) -> Result<Self, SemanticError> {
        let objects = decode(bytes, &expected)?;
        let session = memory_session()?;
        let registry = session.store_registry();
        // Keep this strong reference until the connection takes ownership: registry entries are weak.
        let store = registry
            .get_store(
                "memory://"
                    .parse()
                    .map_err(|_| SemanticError::InvalidIndex)?,
                &Default::default(),
            )
            .await
            .map_err(|_| SemanticError::InvalidIndex)?;
        for object in objects {
            store
                .put(&object.path.into(), object.bytes)
                .await
                .map_err(|_| SemanticError::InvalidIndex)?;
        }
        let connection = lancedb::connect("memory://")
            .session(session.clone())
            .execute()
            .await
            .map_err(|_| SemanticError::InvalidIndex)?;
        let table = connection
            .open_table("crate_vectors")
            .execute()
            .await
            .map_err(|_| SemanticError::InvalidIndex)?;
        let crate_names = validate_table(&table, expected.model.dimension).await?;
        Ok(Self {
            metadata: expected,
            table,
            _connection: connection,
            session,
            crate_names,
        })
    }
}

async fn validate_table(table: &Table, dimension: u32) -> Result<HashSet<String>, SemanticError> {
    let native = table.as_native().ok_or(SemanticError::InvalidIndex)?;
    let manifest = native
        .manifest()
        .await
        .map_err(|_| SemanticError::InvalidIndex)?;
    // This format only represents fresh, complete local generations, never shallow clones or deletes.
    if !manifest.base_paths.is_empty()
        || manifest.fragments.len() > MAX_OBJECTS
        || manifest.fragments.iter().any(|fragment| {
            fragment.deletion_file.is_some()
                || fragment.files.iter().any(|file| {
                    file.base_id.is_some()
                        || !canonical_path(&format!("crate_vectors.lance/{}", file.path))
                })
        })
    {
        return Err(SemanticError::InvalidIndex);
    }
    if table
        .schema()
        .await
        .map_err(|_| SemanticError::InvalidIndex)?
        .as_ref()
        != index_schema(i32::try_from(dimension).map_err(|_| SemanticError::InvalidIndex)?).as_ref()
    {
        return Err(SemanticError::InvalidIndex);
    }
    let count = table
        .count_rows(None)
        .await
        .map_err(|_| SemanticError::InvalidIndex)?;
    if count > MAX_ROWS {
        return Err(SemanticError::Budget);
    }
    let mut stream = table
        .query()
        .limit(MAX_ROWS + 1)
        .execute()
        .await
        .map_err(|_| SemanticError::InvalidIndex)?;
    let mut names = HashSet::with_capacity(count);
    while let Some(batch) = stream
        .try_next()
        .await
        .map_err(|_| SemanticError::InvalidIndex)?
    {
        if batch.num_rows() > count.saturating_sub(names.len()) {
            return Err(SemanticError::InvalidIndex);
        }
        let ids = batch
            .column_by_name("crate_name")
            .and_then(|a| a.as_any().downcast_ref::<StringArray>())
            .ok_or(SemanticError::InvalidIndex)?;
        let vectors = batch
            .column_by_name("vector")
            .and_then(|a| a.as_any().downcast_ref::<FixedSizeListArray>())
            .ok_or(SemanticError::InvalidIndex)?;
        for row in 0..batch.num_rows() {
            if ids.is_null(row)
                || vectors.is_null(row)
                || !valid_crate_name(ids.value(row))
                || !names.insert(ids.value(row).to_owned())
            {
                return Err(SemanticError::InvalidIndex);
            }
            let value = vectors.value(row);
            let value = value
                .as_any()
                .downcast_ref::<Float32Array>()
                .ok_or(SemanticError::InvalidIndex)?;
            if value.null_count() != 0 {
                return Err(SemanticError::InvalidIndex);
            }
            validate_embedding(value.values(), dimension)?;
        }
    }
    if names.len() != count {
        return Err(SemanticError::InvalidIndex);
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{TestResult, metadata, rows};
    use super::*;

    fn archive(
        metadata: &IndexMetadata,
        objects: &[(&str, &[u8])],
    ) -> Result<Vec<u8>, SemanticError> {
        let mut output = MAGIC.to_vec();
        let meta = metadata_bytes(metadata)?;
        number(&mut output, meta.len())?;
        output.extend_from_slice(&meta);
        number(&mut output, objects.len())?;
        for (path, bytes) in objects {
            number(&mut output, path.len())?;
            output.extend_from_slice(path.as_bytes());
            number(&mut output, bytes.len())?;
            output.extend_from_slice(&Sha256::digest(bytes));
            output.extend_from_slice(bytes);
        }
        Ok(output)
    }

    #[tokio::test]
    async fn native_objects_survive_destroying_every_original_handle() -> TestResult {
        let expected = metadata('1', 2)?;
        let index = LanceMemoryIndex::build(expected.clone(), rows()).await?;
        let bytes = index.export().await?;
        let object_names: Vec<_> = decode(&bytes, &expected)?
            .into_iter()
            .map(|o| o.path.to_owned())
            .collect();
        assert!(object_names.iter().any(|p| p.ends_with(".manifest")));
        assert!(object_names.iter().any(|p| p.ends_with(".lance")));
        drop(index);
        let restored = LanceMemoryIndex::restore(&bytes, expected.clone()).await?;
        assert_eq!(restored.metadata(), &expected);
        assert_eq!(
            restored.candidates(&[1.0, 0.0], 3).await?[0].crate_name,
            "serde"
        );
        assert_eq!(restored.export().await?, bytes);
        Ok(())
    }

    #[tokio::test]
    async fn metadata_and_model_are_bound_before_native_open() -> TestResult {
        let original = metadata('1', 2)?;
        let bytes = LanceMemoryIndex::build(original.clone(), rows())
            .await?
            .export()
            .await?;
        for field in 0..5 {
            let mut changed = original.clone();
            match field {
                0 => changed.snapshot_fingerprint = metadata('2', 2)?.snapshot_fingerprint,
                1 => changed.model.revision = "another".into(),
                2 => changed.model.dimension = 3,
                3 => changed.model.runtime = "another".into(),
                _ => changed.model.intra_threads = 3,
            }
            assert!(matches!(
                LanceMemoryIndex::restore(&bytes, changed).await,
                Err(SemanticError::IdentityMismatch)
            ));
        }
        Ok(())
    }

    #[tokio::test]
    async fn corrupt_truncated_and_hostile_archive_keys_fail_closed() -> TestResult {
        let expected = metadata('1', 2)?;
        let bytes = LanceMemoryIndex::build(expected.clone(), rows())
            .await?
            .export()
            .await?;
        for cut in [0, 15, bytes.len() - 1] {
            assert!(matches!(
                LanceMemoryIndex::restore(&bytes[..cut], expected.clone()).await,
                Err(SemanticError::InvalidIndex)
            ));
        }
        let mut corrupt = bytes.clone();
        let last = corrupt.len() - 1;
        corrupt[last] ^= 1;
        assert!(matches!(
            LanceMemoryIndex::restore(&corrupt, expected.clone()).await,
            Err(SemanticError::InvalidIndex)
        ));
        let mut trailing = bytes;
        trailing.push(0);
        assert!(matches!(
            LanceMemoryIndex::restore(&trailing, expected.clone()).await,
            Err(SemanticError::InvalidIndex)
        ));
        for path in [
            "../outside",
            "/crate_vectors.lance/data",
            "crate_vectors.lance/../outside",
            "crate_vectors.lance//a",
            "crate_vectors.lance/https://host/a",
            "crate_vectors.lance/a%2fb",
            "crate_vectors.lance/a\\b",
            "other.lance/data",
        ] {
            let bytes = archive(&expected, &[(path, b"payload")])?;
            assert!(
                matches!(
                    LanceMemoryIndex::restore(&bytes, expected.clone()).await,
                    Err(SemanticError::InvalidIndex)
                ),
                "{path}"
            );
        }
        for objects in [
            vec![("crate_vectors.lance/a", b"a".as_slice()); 2],
            vec![
                ("crate_vectors.lance/b", b"a".as_slice()),
                ("crate_vectors.lance/a", b"a".as_slice()),
            ],
        ] {
            assert!(matches!(
                LanceMemoryIndex::restore(&archive(&expected, &objects)?, expected.clone()).await,
                Err(SemanticError::InvalidIndex)
            ));
        }
        Ok(())
    }

    #[tokio::test]
    async fn memory_registry_cannot_resolve_local_or_remote_references() -> TestResult {
        let session = memory_session()?;
        for uri in [
            "file:///etc/passwd",
            "s3://bucket/data",
            "https://example.org/data",
            "shared-memory://other",
        ] {
            assert!(
                session
                    .store_registry()
                    .get_store(uri.parse()?, &Default::default())
                    .await
                    .is_err()
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn authentic_native_bytes_with_wrong_schema_are_rejected() -> TestResult {
        let expected = metadata('1', 2)?;
        let session = memory_session()?;
        let connection = lancedb::connect("memory://")
            .session(session.clone())
            .execute()
            .await?;
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new(
                "wrong",
                DataType::Utf8,
                false,
            )])),
            vec![Arc::new(StringArray::from(vec!["serde"]))],
        )?;
        let table = connection
            .create_table("crate_vectors", batch)
            .execute()
            .await?;
        let index = LanceMemoryIndex {
            metadata: expected.clone(),
            table,
            _connection: connection,
            session,
            crate_names: HashSet::new(),
        };
        let bytes = index.export().await?;
        drop(index);
        assert!(matches!(
            LanceMemoryIndex::restore(&bytes, expected).await,
            Err(SemanticError::InvalidIndex)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn native_manifest_remote_and_traversal_references_are_rejected() -> TestResult {
        let expected = metadata('1', 2)?;
        let index = LanceMemoryIndex::build(expected.clone(), rows()).await?;
        let manifest = index
            .table
            .as_native()
            .ok_or("native table missing")?
            .manifest()
            .await?;
        let original = manifest.fragments[0].files[0].path.as_bytes();
        let bytes = index.export().await?;
        for prefix in ["https://host/", "../../../"] {
            let mut replacement = prefix.as_bytes().to_vec();
            replacement.resize(original.len(), b'a');
            let objects = decode(&bytes, &expected)?;
            let mut owned: Vec<_> = objects.iter().map(|o| (o.path, o.bytes.to_vec())).collect();
            let mut replaced = false;
            for (path, payload) in &mut owned {
                if path.ends_with(".manifest") {
                    while let Some(at) = payload.windows(original.len()).position(|s| s == original)
                    {
                        payload[at..at + original.len()].copy_from_slice(&replacement);
                        replaced = true;
                    }
                }
            }
            assert!(replaced, "native protobuf data path must be mutated");
            let refs: Vec<_> = owned.iter().map(|(p, b)| (*p, b.as_slice())).collect();
            // Recompute object hashes so this exercises native manifest validation, not checksum rejection.
            let hostile = archive(&expected, &refs)?;
            assert!(matches!(
                LanceMemoryIndex::restore(&hostile, expected.clone()).await,
                Err(SemanticError::InvalidIndex)
            ));
        }
        Ok(())
    }

    #[tokio::test]
    async fn restored_rows_validate_ids_vectors_nulls_and_count() -> TestResult {
        for scenario in 0..6 {
            let expected = metadata('1', 2)?;
            let session = memory_session()?;
            let connection = lancedb::connect("memory://")
                .session(session.clone())
                .execute()
                .await?;
            let count = if scenario == 5 { MAX_ROWS + 1 } else { 2 };
            let names: Vec<String> = (0..count)
                .map(|i| match scenario {
                    0 => "duplicate".to_owned(),
                    1 => "../bad".to_owned(),
                    _ => format!("crate_{i}"),
                })
                .collect();
            let vector = match scenario {
                2 => vec![Some(0.0), Some(0.0)],
                3 => vec![Some(f32::NAN), Some(0.0)],
                4 => vec![None, Some(1.0)],
                _ => vec![Some(1.0), Some(0.0)],
            };
            let vectors = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
                (0..count).map(|_| Some(vector.clone())),
                2,
            );
            let batch = RecordBatch::try_new(
                index_schema(2),
                vec![Arc::new(StringArray::from(names)), Arc::new(vectors)],
            )?;
            let table = connection
                .create_table("crate_vectors", batch)
                .execute()
                .await?;
            let index = LanceMemoryIndex {
                metadata: expected.clone(),
                table,
                _connection: connection,
                session,
                crate_names: HashSet::new(),
            };
            let bytes = index.export().await?;
            drop(index);
            let error = if scenario == 5 {
                SemanticError::Budget
            } else {
                SemanticError::InvalidIndex
            };
            assert!(
                matches!(LanceMemoryIndex::restore(&bytes, expected).await, Err(actual) if actual == error),
                "scenario {scenario}"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn empty_table_round_trips() -> TestResult {
        let expected = metadata('1', 2)?;
        let index = LanceMemoryIndex::build(expected.clone(), vec![]).await?;
        let bytes = index.export().await?;
        drop(index);
        assert!(
            LanceMemoryIndex::restore(&bytes, expected)
                .await?
                .candidates(&[1.0, 0.0], 1)
                .await?
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn archive_budgets_are_enforced_before_native_parsing() -> TestResult {
        let expected = metadata('1', 2)?;
        assert!(matches!(
            decode(&vec![0; MAX_ARCHIVE + 1], &expected),
            Err(SemanticError::Budget)
        ));
        let mut bytes = MAGIC.to_vec();
        number(&mut bytes, MAX_METADATA + 1)?;
        assert!(matches!(
            decode(&bytes, &expected),
            Err(SemanticError::Budget)
        ));
        let mut bytes = archive(&expected, &[])?;
        let count = bytes.len() - 4;
        bytes[count..].copy_from_slice(&129_u32.to_le_bytes());
        assert!(matches!(
            decode(&bytes, &expected),
            Err(SemanticError::Budget)
        ));
        let mut bytes = archive(&expected, &[])?;
        let count = bytes.len() - 4;
        bytes[count..].copy_from_slice(&1_u32.to_le_bytes());
        let path = "crate_vectors.lance/a";
        number(&mut bytes, path.len())?;
        bytes.extend_from_slice(path.as_bytes());
        number(&mut bytes, MAX_OBJECT + 1)?;
        assert!(matches!(
            decode(&bytes, &expected),
            Err(SemanticError::Budget)
        ));
        Ok(())
    }
}
