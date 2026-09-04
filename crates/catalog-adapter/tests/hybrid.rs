//! Real SQLite facts with explicit ready-only retrieval stubs. These tests do not
//! claim to execute an embedding model or a real vector index.
use rust_engineering_application::{
    CatalogRepository, EmbeddingProvider, SemanticIndex, search_catalog, search_hybrid,
};
use rust_engineering_catalog::SqliteCatalogRepository;
use rust_engineering_domain::*;
use std::{
    cell::Cell,
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll, Waker},
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
fn ready<F: Future>(future: F) -> TestResult<F::Output> {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = std::pin::pin!(future);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => Ok(value),
        Poll::Pending => Err("test stub unexpectedly required an async runtime".into()),
    }
}
struct TestClock(Cell<u64>);
impl Clock for TestClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(self.0.get())
    }
}
fn clock() -> TestClock {
    TestClock(Cell::new(1060))
}
fn policy() -> TestResult<FreshnessPolicy> {
    Ok(FreshnessPolicy::new("hybrid-policy".parse()?, 60, 120)?)
}
fn provenance(kind: SourceKind, created: u64) -> TestResult<Provenance> {
    Ok(Provenance::new(
        kind,
        "explicit-test-evidence".parse()?,
        Some(UnixSeconds(created)),
        Some(UnixSeconds(created)),
        IntegrityStatus::Verified,
        false,
    )?)
}
fn version(text: &str, yanked: bool) -> VersionRecord {
    VersionRecord {
        version: text.to_owned(),
        yanked,
        rust_version: Some("1.85".to_owned()),
        license: Some("MIT".to_owned()),
        published_at: Some(900),
        features: vec!["std".to_owned()],
        dependencies: vec![],
        advisories: vec![],
    }
}
fn records() -> Vec<CrateRecord> {
    [
        ("alpha", "needle lexical fact"),
        ("beta", "SQLite authoritative semantic result"),
        ("gamma", "third authoritative result"),
    ]
    .into_iter()
    .map(|(name, description)| CrateRecord {
        name: name.to_owned(),
        description: description.to_owned(),
        repository: None,
        updated_at: Some(999),
        versions: vec![version("1.9.0", false), version("1.10.0", true)],
    })
    .collect()
}
fn repository(input: &[CrateRecord]) -> TestResult<SqliteCatalogRepository> {
    let snapshot =
        SqliteCatalogRepository::build(1, provenance(SourceKind::RegistrySnapshot, 1000)?, input)?;
    Ok(SqliteCatalogRepository::open(
        &snapshot.bytes,
        &snapshot.manifest,
    )?)
}
fn query(limit: u32) -> TestResult<CatalogQuery> {
    Ok(CatalogQuery::new("needle".to_owned(), limit)?)
}

struct Provider {
    identity: EmbeddingIdentity,
    result: Result<Vec<f32>, SemanticError>,
    query_calls: usize,
    passage_calls: usize,
    texts: Vec<String>,
}
impl Provider {
    fn new() -> TestResult<Self> {
        Ok(Self {
            identity: EmbeddingIdentity {
                model: "explicit-stub-not-e5".to_owned(),
                revision: "test-revision-1".to_owned(),
                artifact_fingerprint: format!("sha256:{}", "1".repeat(64)).parse()?,
                runtime: "ready-test-stub".to_owned(),
                provenance: provenance(SourceKind::EmbeddingModel, 950)?,
                dimension: 3,
                max_tokens: 256,
                intra_threads: 1,
                pooling: PoolingKind::Mean,
                normalization: Normalization::L2,
            },
            result: Ok(vec![1.0, 0.0, 0.0]),
            query_calls: 0,
            passage_calls: 0,
            texts: vec![],
        })
    }
}
impl EmbeddingProvider for Provider {
    fn identity(&self) -> &EmbeddingIdentity {
        &self.identity
    }
    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>, SemanticError> {
        self.query_calls += 1;
        self.texts.push(text.to_owned());
        self.result.clone()
    }
    fn embed_passage(&mut self, _: &str) -> Result<Vec<f32>, SemanticError> {
        self.passage_calls += 1;
        Err(SemanticError::InvalidInput)
    }
}
struct Index {
    metadata: IndexMetadata,
    result: Result<Vec<SemanticCandidate>, SemanticError>,
    calls: AtomicUsize,
    requested_limit: AtomicUsize,
}
impl Index {
    fn new(
        repository: &impl CatalogRepository,
        provider: &Provider,
        candidates: Vec<SemanticCandidate>,
    ) -> Self {
        Self {
            metadata: IndexMetadata {
                schema_version: 1,
                snapshot_fingerprint: repository.metadata().fingerprint.clone(),
                model: provider.identity.clone(),
            },
            result: Ok(candidates),
            calls: AtomicUsize::new(0),
            requested_limit: AtomicUsize::new(0),
        }
    }
}
impl SemanticIndex for Index {
    fn metadata(&self) -> &IndexMetadata {
        &self.metadata
    }
    fn candidates<'a>(
        &'a self,
        _: &'a [f32],
        limit: u32,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SemanticCandidate>, SemanticError>> + Send + 'a>>
    {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requested_limit.store(limit as usize, Ordering::SeqCst);
        Box::pin(std::future::ready(self.result.clone()))
    }
}
fn candidate(name: &str, distance: f32) -> SemanticCandidate {
    SemanticCandidate {
        crate_name: name.to_owned(),
        distance,
    }
}

/// Only the authoritative summary read fails; lexical retrieval still uses SQLite.
struct SummaryIntegrityFailure {
    inner: SqliteCatalogRepository,
    summary_calls: Cell<usize>,
}
impl CatalogRepository for SummaryIntegrityFailure {
    fn metadata(&self) -> &CatalogMetadata {
        self.inner.metadata()
    }
    fn lexical(&self, query: &CatalogQuery) -> Result<Vec<CrateSummary>, CatalogError> {
        self.inner.lexical(query)
    }
    fn summary(&self, _: &str) -> Result<Option<CrateSummary>, CatalogError> {
        self.summary_calls.set(self.summary_calls.get() + 1);
        Err(CatalogError::Integrity)
    }
    fn inspect(&self, name: &str) -> Result<Option<CrateRecord>, CatalogError> {
        self.inner.inspect(name)
    }
}

#[test]
fn authoritative_summary_integrity_failure_is_not_a_successful_lexical_fallback() -> TestResult {
    let repository = SummaryIntegrityFailure {
        inner: repository(&records())?,
        summary_calls: Cell::new(0),
    };
    let query = query(2)?;
    let clock = clock();
    let mut provider = Provider::new()?;
    let index = Index::new(&repository, &provider, vec![candidate("beta", 0.1)]);
    // A usable lexical page exists, and the indexed candidate really is in SQLite.
    let lexical = search_catalog(&repository, &query, policy()?, &clock)?;
    assert_eq!(lexical.crates.len(), 1);
    assert_eq!(lexical.crates[0].name, "alpha");
    assert!(repository.inspect("beta")?.is_some());
    let result = ready(search_hybrid(
        &repository,
        &query,
        Some(&mut provider),
        Some(&index),
        policy()?,
        &clock,
    ))?;
    assert_eq!(result, Err(CatalogError::Integrity));
    assert_eq!(repository.summary_calls.get(), 1);
    assert_eq!(provider.query_calls, 1);
    assert_eq!(index.calls.load(Ordering::SeqCst), 1);
    Ok(())
}

fn run(
    repository: &SqliteCatalogRepository,
    query: &CatalogQuery,
    provider: Option<&mut dyn EmbeddingProvider>,
    index: Option<&dyn SemanticIndex>,
    clock: &TestClock,
) -> TestResult<HybridSearch> {
    Ok(ready(search_hybrid(
        repository,
        query,
        provider,
        index,
        policy()?,
        clock,
    ))??)
}
fn fallback(
    actual: &HybridSearch,
    repository: &SqliteCatalogRepository,
    query: &CatalogQuery,
    clock: &TestClock,
    reason: SemanticError,
) -> TestResult {
    assert_eq!(actual.effective_mode, SearchMode::Lexical);
    assert_eq!(actual.fallback, Some(reason));
    assert_eq!(
        actual.results,
        search_catalog(repository, query, policy()?, clock)?
    );
    assert!(actual.semantic_index.is_none());
    assert!(actual.model_evidence.is_none());
    Ok(())
}

#[test]
fn missing_model_or_index_preserves_lexical_and_skips_inference() -> TestResult {
    let repository = repository(&records())?;
    let query = query(2)?;
    let clock = clock();
    let mut provider = Provider::new()?;
    let index = Index::new(&repository, &provider, vec![candidate("beta", 0.2)]);
    fallback(
        &run(&repository, &query, None, Some(&index), &clock)?,
        &repository,
        &query,
        &clock,
        SemanticError::MissingModel,
    )?;
    fallback(
        &run(&repository, &query, Some(&mut provider), None, &clock)?,
        &repository,
        &query,
        &clock,
        SemanticError::MissingIndex,
    )?;
    assert_eq!(provider.query_calls, 0);
    assert_eq!(provider.passage_calls, 0);
    assert_eq!(index.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn snapshot_schema_and_complete_model_identity_are_checked_before_inference() -> TestResult {
    let repository = repository(&records())?;
    let query = query(2)?;
    let clock = clock();
    for field in 0..10 {
        let mut provider = Provider::new()?;
        let mut index = Index::new(&repository, &provider, vec![candidate("beta", 0.2)]);
        match field {
            0 => index.metadata.schema_version = 2,
            1 => {
                index.metadata.snapshot_fingerprint =
                    format!("sha256:{}", "2".repeat(64)).parse()?
            }
            2 => index.metadata.model.model.push_str("-other"),
            3 => index.metadata.model.revision.push_str("-other"),
            4 => {
                index.metadata.model.artifact_fingerprint =
                    format!("sha256:{}", "3".repeat(64)).parse()?
            }
            5 => index.metadata.model.runtime.push_str("-other"),
            6 => index.metadata.model.dimension = 4,
            7 => index.metadata.model.max_tokens = 128,
            8 => index.metadata.model.intra_threads = 2,
            _ => index.metadata.model.provenance = provenance(SourceKind::EmbeddingModel, 951)?,
        }
        let actual = run(
            &repository,
            &query,
            Some(&mut provider),
            Some(&index),
            &clock,
        )?;
        fallback(
            &actual,
            &repository,
            &query,
            &clock,
            SemanticError::IdentityMismatch,
        )?;
        assert_eq!(provider.query_calls, 0, "identity field {field}");
        assert_eq!(index.calls.load(Ordering::SeqCst), 0);
    }
    Ok(())
}

#[test]
fn inference_and_index_errors_preserve_lexical_results_and_explicit_reason() -> TestResult {
    let repository = repository(&records())?;
    let query = query(2)?;
    let clock = clock();
    for reason in [
        SemanticError::Inference,
        SemanticError::InvalidArtifact,
        SemanticError::Budget,
    ] {
        let mut provider = Provider::new()?;
        provider.result = Err(reason);
        let index = Index::new(&repository, &provider, vec![candidate("beta", 0.2)]);
        fallback(
            &run(
                &repository,
                &query,
                Some(&mut provider),
                Some(&index),
                &clock,
            )?,
            &repository,
            &query,
            &clock,
            reason,
        )?;
        assert_eq!(provider.query_calls, 1);
        assert_eq!(index.calls.load(Ordering::SeqCst), 0);
    }
    for reason in [
        SemanticError::MissingIndex,
        SemanticError::InvalidIndex,
        SemanticError::Budget,
    ] {
        let mut provider = Provider::new()?;
        let mut index = Index::new(&repository, &provider, vec![]);
        index.result = Err(reason);
        fallback(
            &run(
                &repository,
                &query,
                Some(&mut provider),
                Some(&index),
                &clock,
            )?,
            &repository,
            &query,
            &clock,
            reason,
        )?;
        assert_eq!(provider.query_calls, 1);
        assert_eq!(provider.passage_calls, 0);
        assert_eq!(index.calls.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

#[test]
fn malformed_embedding_never_reaches_index() -> TestResult {
    let repository = repository(&records())?;
    let query = query(2)?;
    let clock = clock();
    for vector in [
        vec![],
        vec![1.0, 0.0],
        vec![0.0, 0.0, 0.0],
        vec![f32::NAN, 0.0, 0.0],
        vec![f32::INFINITY, 0.0, 0.0],
        vec![1.002, 0.0, 0.0],
    ] {
        let mut provider = Provider::new()?;
        provider.result = Ok(vector);
        let index = Index::new(&repository, &provider, vec![]);
        fallback(
            &run(
                &repository,
                &query,
                Some(&mut provider),
                Some(&index),
                &clock,
            )?,
            &repository,
            &query,
            &clock,
            SemanticError::InvalidIndex,
        )?;
        assert_eq!(provider.query_calls, 1);
        assert_eq!(index.calls.load(Ordering::SeqCst), 0);
    }
    for dimension in [0, 1025] {
        let mut provider = Provider::new()?;
        provider.identity.dimension = dimension;
        let index = Index::new(&repository, &provider, vec![]);
        fallback(
            &run(
                &repository,
                &query,
                Some(&mut provider),
                Some(&index),
                &clock,
            )?,
            &repository,
            &query,
            &clock,
            SemanticError::IdentityMismatch,
        )?;
        assert_eq!(provider.query_calls, 0);
        assert_eq!(index.calls.load(Ordering::SeqCst), 0);
    }
    Ok(())
}

#[test]
fn duplicate_unknown_excessive_or_invalid_distance_candidates_fall_back_atomically() -> TestResult {
    let repository = repository(&records())?;
    let query = query(2)?;
    let clock = clock();
    for candidates in [
        vec![candidate("beta", 0.1), candidate("beta", 0.2)],
        vec![candidate("beta", 0.1), candidate("unknown", 0.2)],
        vec![candidate("beta", 0.1), candidate("' OR 1=1 --", 0.2)],
        vec![candidate("", 0.1)],
        vec![candidate(&"a".repeat(65), 0.1)],
        vec![candidate("craté", 0.1)],
        vec![
            candidate("alpha", 0.0),
            candidate("beta", 0.1),
            candidate("gamma", 0.2),
        ],
        vec![candidate("beta", -0.1)],
        vec![candidate("beta", f32::NAN)],
        vec![candidate("beta", f32::INFINITY)],
        vec![candidate("beta", f32::NEG_INFINITY)],
    ] {
        let mut provider = Provider::new()?;
        let index = Index::new(&repository, &provider, candidates);
        fallback(
            &run(
                &repository,
                &query,
                Some(&mut provider),
                Some(&index),
                &clock,
            )?,
            &repository,
            &query,
            &clock,
            SemanticError::InvalidIndex,
        )?;
        assert_eq!(index.requested_limit.load(Ordering::SeqCst), 2);
    }
    Ok(())
}

#[test]
fn successful_stub_retrieval_deduplicates_and_rehydrates_only_sqlite_facts() -> TestResult {
    let repository = repository(&records())?;
    let query = query(2)?;
    let clock = clock();
    let mut provider = Provider::new()?;
    let index = Index::new(
        &repository,
        &provider,
        vec![candidate("alpha", 0.0), candidate("beta", 0.1)],
    );
    let result = run(
        &repository,
        &query,
        Some(&mut provider),
        Some(&index),
        &clock,
    )?;
    assert_eq!(result.effective_mode, SearchMode::Hybrid);
    assert_eq!(result.fallback, None);
    assert_eq!(result.semantic_index, Some(index.metadata.clone()));
    assert_eq!(
        result.results.crates,
        vec![
            repository.summary("alpha")?.ok_or("alpha absent")?,
            repository.summary("beta")?.ok_or("beta absent")?
        ]
    );
    assert_eq!(
        result.results.crates[1].description,
        "SQLite authoritative semantic result"
    );
    assert_eq!(result.results.crates[1].latest_known.version, "1.10.0");
    assert!(result.results.crates[1].latest_known.yanked);
    assert_eq!(
        result.results.snapshot_fingerprint,
        repository.metadata().fingerprint
    );
    assert_eq!(provider.texts, vec!["needle"]);
    assert_eq!(provider.passage_calls, 0);
    assert_eq!(
        result.results.evidence.freshness().state(),
        FreshnessState::Fresh
    );
    let model = result
        .model_evidence
        .as_ref()
        .ok_or("model evidence absent")?;
    assert_eq!(model.provenance(), &provider.identity.provenance);
    assert_eq!(model.freshness().state(), FreshnessState::Aging);
    assert_eq!(model.freshness().age_seconds(), Some(110));
    clock.0.set(1121);
    let reassessed = run(
        &repository,
        &query,
        Some(&mut provider),
        Some(&index),
        &clock,
    )?;
    assert_eq!(reassessed.results.crates, result.results.crates);
    let model = reassessed
        .model_evidence
        .as_ref()
        .ok_or("model evidence absent")?;
    assert_eq!(model.freshness().state(), FreshnessState::Stale);
    assert_eq!(model.freshness().assessed_at(), UnixSeconds(1121));
    assert!(!model.provenance().network_used());
    Ok(())
}

#[test]
fn semantic_additions_cannot_exceed_query_limit_or_payload_budget() -> TestResult {
    let mut input = records();
    for number in 0..6 {
        input.push(CrateRecord {
            name: format!("large_{number}"),
            description: "x".repeat(4096),
            repository: None,
            updated_at: None,
            versions: vec![version("1.0.0", false)],
        });
    }
    let repository = repository(&input)?;
    let clock = clock();
    let mut provider = Provider::new()?;
    let index = Index::new(&repository, &provider, vec![candidate("beta", 0.1)]);
    let small = query(1)?;
    let result = run(
        &repository,
        &small,
        Some(&mut provider),
        Some(&index),
        &clock,
    )?;
    assert_eq!(result.effective_mode, SearchMode::Hybrid);
    assert_eq!(result.results.crates.len(), 1);
    assert_eq!(result.results.crates[0].name, "alpha");
    let large = query(7)?;
    let index = Index::new(
        &repository,
        &provider,
        (0..6)
            .map(|number| candidate(&format!("large_{number}"), 0.1))
            .collect(),
    );
    fallback(
        &run(
            &repository,
            &large,
            Some(&mut provider),
            Some(&index),
            &clock,
        )?,
        &repository,
        &large,
        &clock,
        SemanticError::Budget,
    )?;
    Ok(())
}
