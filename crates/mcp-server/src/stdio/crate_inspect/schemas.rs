//! Inspection wire-schema mirrors; domain types alone serialize recorded facts.
//! Output contracts use `for_serialize` so Option fields are present and nullable.
#![expect(
    dead_code,
    reason = "Schema-only mirrors are never instantiated; domain values serialize the response"
)]

use schemars::JsonSchema;

type Fingerprint = super::super::toolchain::schemas::Fingerprint;

#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub(super) enum InspectSection {
    Overview,
    Versions,
    Features,
    Dependencies,
    Advisories,
}
#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub(super) enum InspectUnknownReason {
    NotRecordedInSnapshot,
}
#[derive(JsonSchema)]
#[schemars(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum InspectUnknown {
    Unknown { reason: InspectUnknownReason },
}
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct InspectOverview {
    pub name: String,
    pub description: String,
    pub repository: Option<String>,
    pub updated_at: Option<u64>,
    pub latest_known_stable: Option<KnownVersion>,
    pub version_count: u32,
    pub documentation: InspectUnknown,
    pub source: InspectUnknown,
}
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct InspectVersion {
    pub version: String,
    pub yanked: bool,
    pub rust_version: Option<String>,
    pub license: Option<String>,
    pub published_at: Option<u64>,
    pub feature_count: u32,
    pub dependency_count: u32,
    pub advisory_count: u32,
}
#[derive(JsonSchema)]
#[schemars(tag = "section", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum InspectPageData {
    Overview {
        selected_version: Option<InspectVersion>,
    },
    Versions {
        items: Vec<InspectVersion>,
    },
    Features {
        version: InspectVersion,
        items: Vec<String>,
    },
    Dependencies {
        version: InspectVersion,
        items: Vec<DependencyRecord>,
    },
    Advisories {
        version: InspectVersion,
        items: Vec<String>,
    },
}
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct InspectPagination {
    pub offset: u32,
    pub total: u32,
    pub returned: u32,
    pub next_offset: Option<u32>,
    pub omitted_by_output: u32,
}
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct InspectPage {
    pub overview: InspectOverview,
    pub data: InspectPageData,
    pub pagination: InspectPagination,
}
#[derive(JsonSchema)]
#[schemars(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum InspectLookup {
    CrateNotFound,
    VersionNotFound,
    Found { page: Box<InspectPage> },
}
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct CrateInspectResult {
    pub name: String,
    pub snapshot_fingerprint: Fingerprint,
    pub sequence: u64,
    pub evidence: SnapshotEvidence,
    pub lookup: InspectLookup,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct KnownVersion {
    pub version: String,
    pub yanked: bool,
    pub rust_version: Option<String>,
    pub license: Option<String>,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct DependencyRecord {
    pub name: String,
    pub requirement: String,
    pub kind: DependencyKind,
    pub optional: bool,
}
#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub(super) enum DependencyKind {
    Normal,
    Build,
    Dev,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct SnapshotEvidence {
    pub provenance: Provenance,
    pub freshness: Freshness,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct Provenance {
    pub source_kind: SourceKind,
    #[schemars(length(min = 1))]
    pub source_id: String,
    pub created_at: Option<u64>,
    pub observed_at: Option<u64>,
    pub integrity: IntegrityStatus,
    pub network_used: bool,
}

#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub(super) enum SourceKind {
    RegistrySnapshot,
    ProjectSnapshot,
    RustsecSnapshot,
    EmbeddingModel,
    Artifact,
}

#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub(super) enum IntegrityStatus {
    Verified,
    Unverified,
    Failed,
    Unknown,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct Freshness {
    pub state: FreshnessState,
    pub age_seconds: Option<u64>,
    pub assessed_at: u64,
    pub policy: FreshnessPolicy,
}

#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub(super) enum FreshnessState {
    Live,
    Fresh,
    Aging,
    Stale,
    Unknown,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct FreshnessPolicy {
    #[schemars(length(min = 1))]
    pub id: String,
    pub fresh_for_seconds: u64,
    pub stale_after_seconds: u64,
}
