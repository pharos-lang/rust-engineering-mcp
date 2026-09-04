//! Bounded retrieval orchestration. Only the repository selects authoritative facts.
use crate::{CatalogRepository, EmbeddingProvider, InspectionControl, ProjectError, SemanticIndex};
use rust_engineering_domain::*;
use std::collections::{BTreeMap, BTreeSet};

pub trait CatalogSearchRepository: CatalogRepository {
    fn lexical_candidates(
        &self,
        query: &CatalogQuery,
    ) -> Result<Vec<LexicalCandidate>, CatalogError>;
    fn select(
        &self,
        name: &str,
        filters: &CrateSearchFilters,
    ) -> Result<CrateSelection, CatalogError>;
}
pub struct CrateSearchContext<'a> {
    pub repository: &'a dyn CatalogSearchRepository,
    pub provider: Option<&'a mut dyn EmbeddingProvider>,
    pub index: Option<&'a dyn SemanticIndex>,
    pub model_unavailable: Option<CatalogComponentUnavailable>,
    pub index_unavailable: Option<CatalogComponentUnavailable>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogSearchError {
    Project(ProjectError),
    Catalog(CatalogError),
    Unavailable(CatalogComponentUnavailable),
}
impl From<ProjectError> for CatalogSearchError {
    fn from(value: ProjectError) -> Self {
        Self::Project(value)
    }
}
impl From<CatalogError> for CatalogSearchError {
    fn from(value: CatalogError) -> Self {
        Self::Catalog(value)
    }
}
impl std::fmt::Display for CatalogSearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "catalog search failed: {self:?}")
    }
}
impl std::error::Error for CatalogSearchError {}

type SearchResult<T> = Result<T, CatalogSearchError>;
fn name_valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
fn lexical(
    repository: &dyn CatalogSearchRepository,
    query: &CatalogQuery,
    control: &dyn InspectionControl,
) -> SearchResult<Vec<LexicalCandidate>> {
    control.check()?;
    let value = repository.lexical_candidates(query);
    control.check()?;
    let mut value = value?;
    let mut seen = BTreeSet::new();
    if value.len() > SEARCH_CHANNEL_LIMIT as usize
        || value
            .iter()
            .any(|v| !name_valid(&v.name) || !v.bm25.is_finite() || !seen.insert(&v.name))
    {
        return Err(CatalogError::InvalidSnapshot.into());
    }
    value.sort_by(|a, b| {
        // Numeric equality includes signed zero; ties are always name-based.
        if a.bm25 == b.bm25 {
            a.name.cmp(&b.name)
        } else {
            a.bm25.total_cmp(&b.bm25)
        }
    });
    Ok(value)
}

async fn semantic(
    context: &mut CrateSearchContext<'_>,
    query: &CatalogQuery,
    control: &dyn InspectionControl,
) -> SearchResult<Result<(Vec<SemanticCandidate>, IndexMetadata), SearchFallback>> {
    control.check()?;
    let Some(provider) = context.provider.as_deref_mut() else {
        return Ok(Err(SearchFallback::Unavailable {
            component: SearchComponent::Model,
            reason: context
                .model_unavailable
                .unwrap_or(CatalogComponentUnavailable::Missing),
        }));
    };
    let Some(index) = context.index else {
        return Ok(Err(SearchFallback::Unavailable {
            component: SearchComponent::SemanticIndex,
            reason: context
                .index_unavailable
                .unwrap_or(CatalogComponentUnavailable::Missing),
        }));
    };
    let failed = |reason| Ok(Err(SearchFallback::Failed { reason }));
    let metadata = index.metadata();
    if context.model_unavailable.is_some()
        || context.index_unavailable.is_some()
        || metadata.schema_version != 1
        || metadata.model.validate().is_err()
        || metadata.model.provenance.integrity() != IntegrityStatus::Verified
        || metadata.snapshot_fingerprint != context.repository.metadata().fingerprint
        || &metadata.model != provider.identity()
    {
        return failed(SemanticError::IdentityMismatch);
    }
    control.check()?;
    let vector = provider.embed_query(query.text());
    control.check()?;
    let vector = match vector {
        Ok(v) => v,
        Err(e) => return failed(e),
    };
    if validate_embedding(&vector, metadata.model.dimension).is_err() {
        return failed(SemanticError::InvalidIndex);
    }
    control.check()?;
    let candidates = index.candidates(&vector, SEARCH_CHANNEL_LIMIT).await;
    control.check()?;
    let mut candidates = match candidates {
        Ok(v) => v,
        Err(e) => return failed(e),
    };
    let mut seen = BTreeSet::new();
    if candidates.len() > SEARCH_CHANNEL_LIMIT as usize
        || candidates.iter().any(|c| {
            !name_valid(&c.crate_name)
                || !c.distance.is_finite()
                || c.distance < 0.0
                || !seen.insert(&c.crate_name)
        })
    {
        return failed(SemanticError::InvalidIndex);
    }
    candidates.sort_by(|a, b| {
        if a.distance == b.distance {
            a.crate_name.cmp(&b.crate_name)
        } else {
            a.distance.total_cmp(&b.distance)
        }
    });
    Ok(Ok((candidates, metadata.clone())))
}

fn select<'a>(
    cache: &'a mut BTreeMap<String, CrateSelection>,
    repository: &dyn CatalogSearchRepository,
    name: &str,
    filters: &CrateSearchFilters,
    control: &dyn InspectionControl,
) -> SearchResult<&'a CrateSelection> {
    control.check()?;
    if !cache.contains_key(name) {
        if cache.len() >= 100 {
            return Err(CatalogError::Budget.into());
        }
        let selected = repository.select(name, filters);
        control.check()?;
        let selected = selected?;
        if let CrateSelection::Eligible(facts) = &selected {
            validate_facts(name, facts)?;
        }
        cache.insert(name.to_owned(), selected);
    }
    cache.get(name).ok_or(CatalogError::InvalidSnapshot.into())
}
fn validate_facts(name: &str, facts: &SearchCrateFacts) -> Result<(), CatalogError> {
    let optional = |s: &Option<String>, max| {
        s.as_ref()
            .is_none_or(|v| !v.trim().is_empty() && v.len() <= max && !v.contains('\0'))
    };
    let known = |v: &KnownVersion| {
        !v.version.is_empty()
            && v.version.len() <= 128
            && optional(&v.rust_version, 32)
            && optional(&v.license, 512)
    };
    let version = &facts.selected_version;
    let mut ids = BTreeSet::new();
    if facts.name != name
        || facts.description.len() > 4096
        || facts.description.contains('\0')
        || !optional(&facts.repository, 2048)
        || !(1..=64).contains(&facts.version_count)
        || facts
            .latest_known_stable
            .as_ref()
            .is_some_and(|v| !known(v))
        || version.version.is_empty()
        || version.version.len() > 128
        || version.version.contains('\0')
        || !optional(&version.rust_version, 32)
        || !optional(&version.license, 512)
        || version.published_at.is_some_and(|v| v > i64::MAX as u64)
        || version.known_advisory_ids.len() > 128
        || version
            .known_advisory_ids
            .iter()
            .any(|v| !name_valid(v) || !ids.insert(v))
    {
        return Err(CatalogError::InvalidSnapshot);
    }
    Ok(())
}
fn evidence(
    provenance: Provenance,
    id: &str,
    clock: &impl Clock,
) -> SearchResult<SnapshotEvidence> {
    let id = id.parse().map_err(|_| ProjectError::Internal)?;
    let policy = FreshnessPolicy::new(id, 86_400, 604_800).map_err(|_| ProjectError::Internal)?;
    Ok(SnapshotEvidence::assess(provenance, policy, clock))
}

pub async fn search_crates(
    mut context: CrateSearchContext<'_>,
    request: &CrateSearchRequest,
    clock: &impl Clock,
    control: &dyn InspectionControl,
) -> SearchResult<CrateSearchResult> {
    control.check()?;
    let metadata = context.repository.metadata().clone();
    if metadata.sequence == 0
        || metadata.sequence > i64::MAX as u64
        || metadata.provenance.source_kind() != SourceKind::RegistrySnapshot
        || metadata.provenance.integrity() != IntegrityStatus::Verified
        || metadata.provenance.source_id().as_str().len() > 256
    {
        return Err(CatalogError::InvalidSnapshot.into());
    }
    struct Now(UnixSeconds);
    impl Clock for Now {
        fn now(&self) -> UnixSeconds {
            self.0
        }
    }
    let now = Now(clock.now());
    let query = CatalogQuery::new(request.query.text().to_owned(), SEARCH_CHANNEL_LIMIT)?;
    let mut lexical_candidates = if request.mode != CrateSearchMode::Semantic {
        lexical(context.repository, &query, control)?
    } else {
        Vec::new()
    };
    let mut semantic_candidates = Vec::new();
    let mut semantic_index = None;
    let mut fallback = None;
    if request.mode != CrateSearchMode::Lexical {
        match semantic(&mut context, &query, control).await? {
            Ok((candidates, index)) => {
                semantic_candidates = candidates;
                semantic_index = Some(index);
            }
            Err(reason) => fallback = Some(reason),
        }
    }
    let mut cache = BTreeMap::new();
    // Validate every semantic identity even if filters exclude every returned fact.
    // Cache selections so fallback cannot exceed 100 authoritative hydrations.
    for candidate in &semantic_candidates {
        if matches!(
            select(
                &mut cache,
                context.repository,
                &candidate.crate_name,
                &request.filters,
                control
            )?,
            CrateSelection::Missing
        ) {
            fallback = Some(SearchFallback::Failed {
                reason: SemanticError::InvalidIndex,
            });
            break;
        }
    }
    let effective_mode = if fallback.is_some() {
        CrateSearchMode::Lexical
    } else {
        request.mode
    };
    if fallback.is_some() {
        control.check()?;
        semantic_candidates.clear();
        semantic_index = None;
        if request.mode == CrateSearchMode::Semantic {
            lexical_candidates = lexical(context.repository, &query, control)?;
        }
    }
    let mut scores = BTreeMap::<String, (Option<LexicalScore>, Option<SemanticScore>)>::new();
    for (rank, candidate) in lexical_candidates.iter().enumerate() {
        scores.entry(candidate.name.clone()).or_default().0 = Some(LexicalScore {
            rank: rank as u32 + 1,
            bm25: candidate.bm25,
        });
    }
    for (rank, candidate) in semantic_candidates.iter().enumerate() {
        scores.entry(candidate.crate_name.clone()).or_default().1 = Some(SemanticScore {
            rank: rank as u32 + 1,
            squared_l2: candidate.distance,
        });
    }
    let examined = scores.len() as u32;
    let mut results = Vec::with_capacity(scores.len());
    let mut filtered_out = 0;
    for (name, (lexical, semantic)) in scores {
        match select(
            &mut cache,
            context.repository,
            &name,
            &request.filters,
            control,
        )? {
            CrateSelection::Missing => return Err(CatalogError::InvalidSnapshot.into()),
            CrateSelection::FilteredOut => filtered_out += 1,
            CrateSelection::Eligible(facts) => {
                let fusion_score = (effective_mode == CrateSearchMode::Hybrid).then(|| {
                    lexical
                        .as_ref()
                        .map_or(0.0, |s| 1.0 / (60.0 + f64::from(s.rank)))
                        + semantic
                            .as_ref()
                            .map_or(0.0, |s| 1.0 / (60.0 + f64::from(s.rank)))
                });
                results.push(RankedCrate {
                    facts: (**facts).clone(),
                    lexical,
                    semantic,
                    fusion_score,
                });
            }
        }
        control.check()?;
    }
    results.sort_by(|a, b| {
        let rank = match effective_mode {
            CrateSearchMode::Hybrid => b
                .fusion_score
                .unwrap_or(0.0)
                .total_cmp(&a.fusion_score.unwrap_or(0.0)),
            CrateSearchMode::Lexical => a
                .lexical
                .as_ref()
                .map(|s| s.rank)
                .cmp(&b.lexical.as_ref().map(|s| s.rank)),
            CrateSearchMode::Semantic => a
                .semantic
                .as_ref()
                .map(|s| s.rank)
                .cmp(&b.semantic.as_ref().map(|s| s.rank)),
        };
        rank.then_with(|| a.facts.name.cmp(&b.facts.name))
    });
    let eligible = results.len() as u32;
    results.truncate(request.query.limit() as usize);
    let returned = results.len() as u32;
    let model_evidence = semantic_index
        .as_ref()
        .map(|v| evidence(v.model.provenance.clone(), "catalog-model-v1", &now))
        .transpose()?;
    let result = CrateSearchResult {
        requested_mode: request.mode,
        effective_mode,
        fallback,
        snapshot_fingerprint: metadata.fingerprint,
        evidence: evidence(metadata.provenance, "catalog-snapshot-v1", &now)?,
        semantic_index,
        model_evidence,
        results,
        window: SearchWindow {
            candidate_limit_per_channel: SEARCH_CHANNEL_LIMIT,
            lexical_candidates: lexical_candidates.len() as u32,
            semantic_candidates: semantic_candidates.len() as u32,
            examined,
            filtered_out,
            eligible,
            returned,
            limit_truncated: eligible - returned,
            omitted_by_output: 0,
        },
    };
    control.check()?;
    Ok(result)
}
