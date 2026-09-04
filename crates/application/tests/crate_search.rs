use rust_engineering_application::*;
use rust_engineering_domain::*;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn ready<F: Future>(future: F) -> Result<F::Output, Box<dyn std::error::Error>> {
    let mut context = Context::from_waker(Waker::noop());
    match std::pin::pin!(future).poll(&mut context) {
        Poll::Ready(value) => Ok(value),
        Poll::Pending => Err("test future unexpectedly pending".into()),
    }
}
struct Control(Arc<AtomicBool>);
impl OperationControl for Control {
    fn check(&self) -> Result<(), ProjectError> {
        if self.0.load(Ordering::SeqCst) {
            Err(ProjectError::Cancelled)
        } else {
            Ok(())
        }
    }
}
impl ExecutionCancellation for Control {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}
struct ClockNow;
impl Clock for ClockNow {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(100_000)
    }
}
fn fingerprint() -> Result<CatalogFingerprint, Box<dyn std::error::Error>> {
    Ok(format!("sha256:{:064x}", 1).parse()?)
}
fn provenance(kind: SourceKind) -> Result<Provenance, Box<dyn std::error::Error>> {
    Ok(Provenance::new(
        kind,
        "fixture".parse()?,
        Some(UnixSeconds(1)),
        Some(UnixSeconds(2)),
        IntegrityStatus::Verified,
        true,
    )?)
}
fn facts(name: &str) -> SearchCrateFacts {
    SearchCrateFacts {
        name: name.into(),
        description: format!("SQLite {name}"),
        repository: Some("https://example.invalid/repo".into()),
        latest_known_stable: None,
        selected_version: SearchVersionFacts {
            version: "1.0.0".into(),
            yanked: false,
            rust_version: Some("1.80".into()),
            license: Some("MIT".into()),
            published_at: Some(1),
            known_advisory_ids: vec![],
        },
        version_count: 1,
    }
}
struct Repository {
    metadata: CatalogMetadata,
    lexical: Vec<LexicalCandidate>,
    records: BTreeMap<String, CrateSelection>,
    calls: Cell<u32>,
    selected: RefCell<Vec<(String, CrateSearchFilters)>>,
    select_error: Option<CatalogError>,
    cancel_on_select: Option<Arc<AtomicBool>>,
}
impl CatalogRepository for Repository {
    fn metadata(&self) -> &CatalogMetadata {
        &self.metadata
    }
    fn lexical(&self, _: &CatalogQuery) -> Result<Vec<CrateSummary>, CatalogError> {
        Err(CatalogError::InvalidInput)
    }
    fn summary(&self, _: &str) -> Result<Option<CrateSummary>, CatalogError> {
        Err(CatalogError::InvalidInput)
    }
    fn inspect(&self, _: &str) -> Result<Option<CrateRecord>, CatalogError> {
        Err(CatalogError::InvalidInput)
    }
}
impl CatalogSearchRepository for Repository {
    fn lexical_candidates(
        &self,
        query: &CatalogQuery,
    ) -> Result<Vec<LexicalCandidate>, CatalogError> {
        assert_eq!(query.limit(), 50);
        self.calls.set(self.calls.get() + 1);
        Ok(self.lexical.clone())
    }
    fn select(
        &self,
        name: &str,
        filters: &CrateSearchFilters,
    ) -> Result<CrateSelection, CatalogError> {
        self.selected
            .borrow_mut()
            .push((name.into(), filters.clone()));
        if let Some(cancel) = &self.cancel_on_select {
            cancel.store(true, Ordering::SeqCst);
        }
        if let Some(error) = self.select_error {
            return Err(error);
        }
        Ok(self
            .records
            .get(name)
            .cloned()
            .unwrap_or(CrateSelection::Missing))
    }
}
struct Model {
    identity: EmbeddingIdentity,
    calls: Arc<AtomicUsize>,
    error: Option<SemanticError>,
    cancel: Option<Arc<AtomicBool>>,
    vector: Vec<f32>,
}
impl EmbeddingProvider for Model {
    fn identity(&self) -> &EmbeddingIdentity {
        &self.identity
    }
    fn embed_query(&mut self, _: &str) -> Result<Vec<f32>, SemanticError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(cancel) = &self.cancel {
            cancel.store(true, Ordering::SeqCst);
        }
        self.error.map_or_else(|| Ok(self.vector.clone()), Err)
    }
    fn embed_passage(&mut self, _: &str) -> Result<Vec<f32>, SemanticError> {
        Err(SemanticError::InvalidInput)
    }
}
struct Index {
    metadata: IndexMetadata,
    candidates: Vec<SemanticCandidate>,
    error: Option<SemanticError>,
    cancel: Option<Arc<AtomicBool>>,
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
        Box::pin(async move {
            assert_eq!(limit, 50);
            if let Some(cancel) = &self.cancel {
                cancel.store(true, Ordering::SeqCst);
            }
            self.error.map_or_else(|| Ok(self.candidates.clone()), Err)
        })
    }
}
struct Harness {
    repository: Repository,
    model: Model,
    index: Index,
    control: Control,
}
impl Harness {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let identity = EmbeddingIdentity {
            model: "fixture-e5".into(),
            revision: "fixed".into(),
            artifact_fingerprint: fingerprint()?,
            runtime: "test".into(),
            provenance: provenance(SourceKind::EmbeddingModel)?,
            dimension: 2,
            max_tokens: 512,
            intra_threads: 2,
            pooling: PoolingKind::Mean,
            normalization: Normalization::L2,
        };
        Ok(Self {
            repository: Repository {
                metadata: CatalogMetadata {
                    sequence: 1,
                    fingerprint: fingerprint()?,
                    provenance: provenance(SourceKind::RegistrySnapshot)?,
                },
                lexical: vec![LexicalCandidate {
                    name: "lexical".into(),
                    bm25: -1.0,
                }],
                records: ["lexical", "semantic", "both"]
                    .into_iter()
                    .map(|name| (name.into(), CrateSelection::Eligible(Box::new(facts(name)))))
                    .collect(),
                calls: Cell::new(0),
                selected: RefCell::new(vec![]),
                select_error: None,
                cancel_on_select: None,
            },
            index: Index {
                metadata: IndexMetadata {
                    schema_version: 1,
                    snapshot_fingerprint: fingerprint()?,
                    model: identity.clone(),
                },
                candidates: vec![SemanticCandidate {
                    crate_name: "semantic".into(),
                    distance: 0.0,
                }],
                error: None,
                cancel: None,
            },
            model: Model {
                identity,
                calls: Arc::new(AtomicUsize::new(0)),
                error: None,
                cancel: None,
                vector: vec![1.0, 0.0],
            },
            control: Control(Arc::new(AtomicBool::new(false))),
        })
    }
    fn run(
        &mut self,
        mode: CrateSearchMode,
        limit: u32,
    ) -> Result<Result<CrateSearchResult, CatalogSearchError>, Box<dyn std::error::Error>> {
        let request = CrateSearchRequest {
            query: CatalogQuery::new("crate".into(), limit)?,
            mode,
            filters: CrateSearchFilters::default(),
        };
        ready(search_crates(
            CrateSearchContext {
                repository: &self.repository,
                provider: Some(&mut self.model),
                index: Some(&self.index),
                model_unavailable: None,
                index_unavailable: None,
            },
            &request,
            &ClockNow,
            &self.control,
        ))
    }
}
#[test]
fn modes_use_only_requested_channels_and_preserve_scores() -> TestResult {
    let mut h = Harness::new()?;
    let lexical = h.run(CrateSearchMode::Lexical, 10)??;
    assert_eq!(h.model.calls.load(Ordering::SeqCst), 0);
    assert_eq!(lexical.results[0].facts.name, "lexical");
    assert!(lexical.semantic_index.is_none());
    assert!(lexical.results[0].semantic.is_none());
    assert_eq!(
        lexical.results[0].lexical.as_ref().map(|v| v.bm25),
        Some(-1.0)
    );
    h.repository.calls.set(0);
    let semantic = h.run(CrateSearchMode::Semantic, 10)??;
    assert_eq!(h.repository.calls.get(), 0);
    assert_eq!(semantic.results[0].facts.description, "SQLite semantic");
    assert!(semantic.results[0].lexical.is_none());
    assert_eq!(
        semantic.results[0].semantic.as_ref().map(|v| v.squared_l2),
        Some(0.0)
    );
    assert!(semantic.model_evidence.is_some());
    assert!(semantic.evidence.provenance().network_used());
    Ok(())
}
#[test]
fn full_lexical_window_cannot_suppress_hybrid_fusion() -> TestResult {
    let mut h = Harness::new()?;
    h.repository.lexical = (0..50)
        .map(|n| LexicalCandidate {
            name: format!("a{n:02}"),
            bm25: f64::from(n),
        })
        .collect();
    for n in 0..50 {
        let name = format!("a{n:02}");
        h.repository.records.insert(
            name.clone(),
            CrateSelection::Eligible(Box::new(facts(&name))),
        );
    }
    h.index.candidates = vec![
        SemanticCandidate {
            crate_name: "semantic".into(),
            distance: 0.0,
        },
        SemanticCandidate {
            crate_name: "a49".into(),
            distance: 1.0,
        },
    ];
    let result = h.run(CrateSearchMode::Hybrid, 1)??;
    assert_eq!(result.results[0].facts.name, "a49");
    assert_eq!(
        result.results[0].fusion_score,
        Some(1.0 / 110.0 + 1.0 / 62.0)
    );
    assert_eq!(
        (
            result.window.examined,
            result.window.eligible,
            result.window.returned,
            result.window.limit_truncated
        ),
        (51, 51, 1, 50)
    );
    assert_eq!(h.repository.selected.borrow().len(), 51);
    Ok(())
}
#[test]
fn tie_order_is_name_deterministic_and_filters_precede_limit() -> TestResult {
    let mut h = Harness::new()?;
    h.repository.lexical = vec![
        LexicalCandidate {
            name: "semantic".into(),
            bm25: 0.0,
        },
        LexicalCandidate {
            name: "both".into(),
            bm25: 0.0,
        },
        LexicalCandidate {
            name: "lexical".into(),
            bm25: 0.0,
        },
    ];
    h.repository
        .records
        .insert("both".into(), CrateSelection::FilteredOut);
    let filters = CrateSearchFilters {
        msrv_lte: Some(MsrvVersion::parse("1.80")?),
        allow_yanked: true,
        include_prerelease: true,
    };
    let request = CrateSearchRequest {
        query: CatalogQuery::new("x".into(), 1)?,
        mode: CrateSearchMode::Lexical,
        filters: filters.clone(),
    };
    let result = ready(search_crates(
        CrateSearchContext {
            repository: &h.repository,
            provider: None,
            index: None,
            model_unavailable: None,
            index_unavailable: None,
        },
        &request,
        &ClockNow,
        &h.control,
    ))??;
    assert_eq!(result.results[0].facts.name, "lexical");
    assert_eq!(result.results[0].lexical.as_ref().map(|s| s.rank), Some(2));
    assert_eq!(
        (
            result.window.examined,
            result.window.filtered_out,
            result.window.eligible
        ),
        (3, 1, 2)
    );
    assert!(
        h.repository
            .selected
            .borrow()
            .iter()
            .all(|(_, f)| f == &filters)
    );
    Ok(())
}
#[test]
fn invalid_semantic_candidates_and_identity_trigger_explicit_fallback() -> TestResult {
    for case in 0..8 {
        let mut h = Harness::new()?;
        match case {
            0 => h.index.candidates.push(h.index.candidates[0].clone()),
            1 => h.index.candidates[0].distance = f32::NAN,
            2 => h.index.candidates[0].distance = -1.0,
            3 => h.index.candidates[0].crate_name = "unknown".into(),
            4 => h.index.metadata.schema_version = 2,
            5 => h.model.error = Some(SemanticError::Inference),
            6 => h.index.error = Some(SemanticError::Budget),
            _ => h.model.vector = vec![0.0, 0.0],
        }
        let result = h.run(CrateSearchMode::Semantic, 10)??;
        assert_eq!(result.effective_mode, CrateSearchMode::Lexical);
        assert!(matches!(
            result.fallback,
            Some(SearchFallback::Failed { .. })
        ));
        assert!(result.semantic_index.is_none() && result.model_evidence.is_none());
        assert_eq!(result.results[0].facts.name, "lexical");
        assert_eq!(result.window.semantic_candidates, 0);
    }
    Ok(())
}
#[test]
fn unknown_semantic_identity_fails_even_when_all_other_candidates_filtered() -> TestResult {
    let mut h = Harness::new()?;
    h.repository
        .records
        .insert("semantic".into(), CrateSelection::FilteredOut);
    h.index.candidates.push(SemanticCandidate {
        crate_name: "unknown".into(),
        distance: 1.0,
    });
    let result = h.run(CrateSearchMode::Hybrid, 10)??;
    assert_eq!(
        result.fallback,
        Some(SearchFallback::Failed {
            reason: SemanticError::InvalidIndex
        })
    );
    assert_eq!(result.results[0].facts.name, "lexical");
    Ok(())
}
#[test]
fn missing_components_retain_typed_reason_and_same_filters() -> TestResult {
    let h = Harness::new()?;
    let request = CrateSearchRequest {
        query: CatalogQuery::new("x".into(), 10)?,
        mode: CrateSearchMode::Semantic,
        filters: CrateSearchFilters {
            msrv_lte: Some(MsrvVersion::parse("1.2")?),
            ..Default::default()
        },
    };
    let result = ready(search_crates(
        CrateSearchContext {
            repository: &h.repository,
            provider: None,
            index: None,
            model_unavailable: Some(CatalogComponentUnavailable::FeatureDisabled),
            index_unavailable: Some(CatalogComponentUnavailable::DependencyUnavailable),
        },
        &request,
        &ClockNow,
        &h.control,
    ))??;
    assert_eq!(
        result.fallback,
        Some(SearchFallback::Unavailable {
            component: SearchComponent::Model,
            reason: CatalogComponentUnavailable::FeatureDisabled
        })
    );
    assert!(
        h.repository
            .selected
            .borrow()
            .iter()
            .all(|(_, filters)| filters == &request.filters)
    );
    Ok(())
}
#[test]
fn cancellation_after_native_failure_or_hydration_never_falls_back() -> TestResult {
    for stage in 0..3 {
        let mut h = Harness::new()?;
        match stage {
            0 => {
                h.model.cancel = Some(h.control.0.clone());
                h.model.error = Some(SemanticError::Inference);
            }
            1 => {
                h.index.cancel = Some(h.control.0.clone());
                h.index.error = Some(SemanticError::Budget);
            }
            _ => h.repository.cancel_on_select = Some(h.control.0.clone()),
        }
        assert_eq!(
            h.run(CrateSearchMode::Semantic, 10)?,
            Err(CatalogSearchError::Project(ProjectError::Cancelled))
        );
        assert_eq!(h.repository.calls.get(), 0);
    }
    Ok(())
}
#[test]
fn sqlite_failures_and_invalid_lexical_candidates_are_not_semantic_fallback() -> TestResult {
    let mut h = Harness::new()?;
    h.repository.select_error = Some(CatalogError::Budget);
    assert_eq!(
        h.run(CrateSearchMode::Semantic, 10)?,
        Err(CatalogSearchError::Catalog(CatalogError::Budget))
    );
    h.repository.select_error = None;
    h.repository.lexical[0].bm25 = f64::INFINITY;
    assert_eq!(
        h.run(CrateSearchMode::Lexical, 10)?,
        Err(CatalogSearchError::Catalog(CatalogError::InvalidSnapshot))
    );
    Ok(())
}
#[test]
fn fallback_hydrates_at_most_the_hundred_candidate_union() -> TestResult {
    let mut h = Harness::new()?;
    h.repository.lexical = (0..50)
        .map(|n| LexicalCandidate {
            name: format!("a{n:02}"),
            bm25: f64::from(n),
        })
        .collect();
    h.index.candidates = (0..50)
        .map(|n| SemanticCandidate {
            crate_name: format!("z{n:02}"),
            distance: n as f32,
        })
        .collect();
    for prefix in ["a", "z"] {
        for n in 0..50 {
            let name = format!("{prefix}{n:02}");
            h.repository.records.insert(
                name.clone(),
                CrateSelection::Eligible(Box::new(facts(&name))),
            );
        }
    }
    h.repository.records.remove("z49");
    let result = h.run(CrateSearchMode::Semantic, 50)??;
    assert_eq!(result.effective_mode, CrateSearchMode::Lexical);
    assert_eq!(h.repository.selected.borrow().len(), 100);
    assert_eq!(result.window.examined, 50);
    Ok(())
}

#[test]
fn matching_unverified_model_and_index_do_not_establish_semantic_integrity() -> TestResult {
    let mut h = Harness::new()?;
    h.model.identity.provenance = Provenance::new(
        SourceKind::EmbeddingModel,
        "unverified".parse()?,
        Some(UnixSeconds(1)),
        Some(UnixSeconds(2)),
        IntegrityStatus::Unverified,
        true,
    )?;
    h.index.metadata.model = h.model.identity.clone();
    let result = h.run(CrateSearchMode::Semantic, 10)??;
    assert_eq!(
        result.fallback,
        Some(SearchFallback::Failed {
            reason: SemanticError::IdentityMismatch
        })
    );
    assert_eq!(h.model.calls.load(Ordering::SeqCst), 0);
    assert!(result.model_evidence.is_none());
    Ok(())
}

#[test]
fn signed_zero_scores_tie_by_name_in_each_channel() -> TestResult {
    let mut h = Harness::new()?;
    h.repository.lexical = vec![
        LexicalCandidate {
            name: "semantic".into(),
            bm25: -0.0,
        },
        LexicalCandidate {
            name: "both".into(),
            bm25: 0.0,
        },
    ];
    h.index.candidates = vec![
        SemanticCandidate {
            crate_name: "semantic".into(),
            distance: -0.0,
        },
        SemanticCandidate {
            crate_name: "both".into(),
            distance: 0.0,
        },
    ];
    for mode in [CrateSearchMode::Lexical, CrateSearchMode::Semantic] {
        let result = h.run(mode, 10)??;
        assert_eq!(result.results[0].facts.name, "both");
        assert_eq!(result.results[1].facts.name, "semantic");
    }
    Ok(())
}
