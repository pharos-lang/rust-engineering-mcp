//! Read-only session generation. Administration and acquisition stay in the CLI.
use super::super::auditing::provider::HostAuditConfig;
#[cfg(feature = "local")]
use rust_engineering_application::EmbeddingProvider;
use rust_engineering_application::{
    CatalogRepository, CatalogStatusPort, InspectionControl, ProjectError,
};
use rust_engineering_catalog::{
    RustSecSnapshot,
    bundle::{self, PublisherTrust, SequenceFloor, VerifiedBundle},
};
use rust_engineering_domain::*;
use rust_engineering_project::catalog_store::{self, StoreError};
use std::{path::PathBuf, sync::Mutex};

#[derive(Clone)]
pub struct HostCatalogConfig {
    pub store: PathBuf,
    pub trust: PathBuf,
    pub model_dir: Option<PathBuf>,
    pub index_store: Option<PathBuf>,
}
pub(crate) struct CatalogProvider {
    host: Option<HostCatalogConfig>,
    audit: Option<HostAuditConfig>,
    state: Mutex<Option<LoadedCatalog>>,
}
struct LoadedCatalog {
    observation: CatalogContextObservation,
    // Retained verified handles are the immutable session generation, not a second load.
    _bundle: Option<VerifiedBundle>,
    #[cfg(feature = "local")]
    _model: Option<rust_engineering_semantic::LocalEmbeddingProvider>,
    #[cfg(feature = "local")]
    _index: Option<rust_engineering_semantic::LanceMemoryIndex>,
}
fn unavailable<T>(reason: CatalogComponentUnavailable) -> Component<T> {
    Component::Unavailable { reason }
}
impl CatalogProvider {
    pub(crate) fn new(host: Option<HostCatalogConfig>, audit: Option<HostAuditConfig>) -> Self {
        Self {
            host,
            audit,
            state: Mutex::new(None),
        }
    }
    pub(crate) fn search(
        &self,
        request: &CrateSearchRequest,
        clock: &impl Clock,
        control: &dyn InspectionControl,
    ) -> Result<CrateSearchResult, rust_engineering_application::CatalogSearchError> {
        use rust_engineering_application::{CatalogSearchError, CrateSearchContext, search_crates};
        control.check().map_err(CatalogSearchError::Project)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| CatalogSearchError::Project(ProjectError::Internal))?;
        if state.is_none() {
            *state = Some(self.load(control).map_err(CatalogSearchError::Project)?);
        }
        let loaded = state
            .as_mut()
            .ok_or(CatalogSearchError::Project(ProjectError::Internal))?;
        let missing = |component: &Component<_>| match component {
            Component::Unavailable { reason } => Some(*reason),
            Component::Available { .. } => None,
        };
        let catalog_unavailable = missing(&loaded.observation.catalog);
        let bundle = loaded
            ._bundle
            .as_ref()
            .ok_or(CatalogSearchError::Unavailable(
                catalog_unavailable.unwrap_or(CatalogComponentUnavailable::Invalid),
            ))?;
        let model_unavailable = match &loaded.observation.model {
            Component::Unavailable { reason } => Some(*reason),
            _ => None,
        };
        let index_unavailable = match &loaded.observation.semantic_index {
            Component::Unavailable { reason } => Some(*reason),
            _ => None,
        };
        #[cfg(feature = "local")]
        let (provider, index) = (
            loaded
                ._model
                .as_mut()
                .map(|p| p as &mut dyn rust_engineering_application::EmbeddingProvider),
            loaded
                ._index
                .as_ref()
                .map(|p| p as &dyn rust_engineering_application::SemanticIndex),
        );
        #[cfg(not(feature = "local"))]
        let (provider, index) = (None, None);
        let context = CrateSearchContext {
            repository: bundle.repository(),
            provider,
            index,
            model_unavailable,
            index_unavailable,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| CatalogSearchError::Project(ProjectError::Internal))?;
        runtime.block_on(search_crates(context, request, clock, control))
    }
    pub(crate) fn inspect(
        &self,
        request: &CrateInspectRequest,
        clock: &impl Clock,
        control: &dyn InspectionControl,
    ) -> Result<CrateInspectResult, rust_engineering_application::CatalogInspectError> {
        use rust_engineering_application::{CatalogInspectError, inspect_crate};
        control.check().map_err(CatalogInspectError::Project)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| CatalogInspectError::Project(ProjectError::Internal))?;
        if state.is_none() {
            *state = Some(self.load(control).map_err(CatalogInspectError::Project)?);
        }
        let loaded = state
            .as_ref()
            .ok_or(CatalogInspectError::Project(ProjectError::Internal))?;
        let reason = match &loaded.observation.catalog {
            Component::Unavailable { reason } => *reason,
            _ => CatalogComponentUnavailable::Invalid,
        };
        let bundle = loaded
            ._bundle
            .as_ref()
            .ok_or(CatalogInspectError::Unavailable(reason))?;
        inspect_crate(bundle.repository(), request, clock, control)
    }
    fn load(&self, control: &dyn InspectionControl) -> Result<LoadedCatalog, ProjectError> {
        use CatalogComponentUnavailable as U;
        let mut loaded = LoadedCatalog {
            observation: CatalogContextObservation {
                catalog: unavailable(U::NotConfigured),
                reservation: None,
                model: unavailable(U::NotConfigured),
                semantic_index: unavailable(U::NotConfigured),
                rustsec: unavailable(U::NotConfigured),
            },
            _bundle: None,
            #[cfg(feature = "local")]
            _model: None,
            #[cfg(feature = "local")]
            _index: None,
        };
        let Some(host) = &self.host else {
            return Ok(loaded);
        };
        let acquired = self.read_catalog(host, &mut loaded.observation, control)?;
        loaded._bundle = acquired;
        control.check()?;
        self.load_semantics(host, &mut loaded, control)?;
        control.check()?;
        Ok(loaded)
    }
    fn read_catalog(
        &self,
        host: &HostCatalogConfig,
        observation: &mut CatalogContextObservation,
        control: &dyn InspectionControl,
    ) -> Result<Option<VerifiedBundle>, ProjectError> {
        let result = (|| {
            let trust = catalog_store::read_private_optional_file(&host.trust, 4096)
                .map_err(store_error)?
                .ok_or(CatalogComponentUnavailable::Missing)?;
            let trust =
                PublisherTrust::parse(&trust).map_err(|_| CatalogComponentUnavailable::Invalid)?;
            for _ in 0..3 {
                if control.check().is_err() {
                    return Err(CatalogComponentUnavailable::Budget);
                }
                // A floor change forces a retry, never a mixed-generation availability claim.
                let before = catalog_store::read_private_optional_file(
                    &host.store.join("floor.record"),
                    4096,
                )
                .map_err(store_error)?;
                let active = catalog_store::read_private_optional_file(
                    &host.store.join("active.bundle"),
                    bundle::MAX_BUNDLE_BYTES,
                )
                .map_err(store_error)?;
                if control.check().is_err() {
                    return Err(CatalogComponentUnavailable::Budget);
                }
                let after = catalog_store::read_private_optional_file(
                    &host.store.join("floor.record"),
                    4096,
                )
                .map_err(store_error)?;
                if before != after {
                    continue;
                }
                let floor = before
                    .map(|bytes| SequenceFloor::parse(&bytes, &trust))
                    .transpose()
                    .map_err(|error| match error {
                        bundle::FloorError::InvalidState => CatalogComponentUnavailable::Invalid,
                        bundle::FloorError::TrustMismatch => {
                            CatalogComponentUnavailable::IdentityMismatch
                        }
                    })?;
                if let Some(floor) = &floor {
                    observation.reservation = Some(CatalogReservation {
                        publisher: floor.publisher().to_owned(),
                        channel: floor.channel().to_owned(),
                        sequence: floor.sequence(),
                        bundle_fingerprint: format!("sha256:{}", floor.bundle_sha256())
                            .parse()
                            .map_err(|_| CatalogComponentUnavailable::Invalid)?,
                    });
                }
                let Some(active) = active else {
                    return Err(CatalogComponentUnavailable::Missing);
                };
                let floor = floor.ok_or(CatalogComponentUnavailable::Invalid)?;
                if control.check().is_err() {
                    return Err(CatalogComponentUnavailable::Budget);
                }
                let active = bundle::verify(&active, &trust)
                    .map_err(|_| CatalogComponentUnavailable::Invalid)?;
                if active.manifest().sequence > floor.sequence()
                    || (active.manifest().sequence == floor.sequence() && !floor.matches(&active))
                {
                    return Err(CatalogComponentUnavailable::IdentityMismatch);
                }
                let count = active
                    .repository()
                    .embedding_documents()
                    .map_err(|_| CatalogComponentUnavailable::Invalid)?
                    .len();
                observation.catalog = Component::Available {
                    value: CatalogContextCatalogObservation {
                        publisher: trust.publisher.clone(),
                        channel: trust.channel.clone(),
                        publisher_key_fingerprint: format!(
                            "sha256:{}",
                            trust
                                .key_fingerprint()
                                .map_err(|_| CatalogComponentUnavailable::Invalid)?
                        )
                        .parse()
                        .map_err(|_| CatalogComponentUnavailable::Invalid)?,
                        bundle_fingerprint: format!("sha256:{}", active.fingerprint())
                            .parse()
                            .map_err(|_| CatalogComponentUnavailable::Invalid)?,
                        metadata: active.repository().metadata().clone(),
                        schema_version: active.manifest().catalog_schema_version,
                        crate_count: count
                            .try_into()
                            .map_err(|_| CatalogComponentUnavailable::Budget)?,
                        bundled_rustsec_available: active.rustsec_bytes().is_some(),
                    },
                };
                return Ok(active);
            }
            Err(CatalogComponentUnavailable::IoUnavailable)
        })();
        control.check()?;
        match result {
            Ok(value) => Ok(Some(value)),
            Err(reason) => {
                observation.catalog = unavailable(reason);
                Ok(None)
            }
        }
    }
    #[cfg(not(feature = "local"))]
    fn load_semantics(
        &self,
        host: &HostCatalogConfig,
        loaded: &mut LoadedCatalog,
        _: &dyn InspectionControl,
    ) -> Result<(), ProjectError> {
        if host.model_dir.is_some() {
            loaded.observation.model = unavailable(CatalogComponentUnavailable::FeatureDisabled);
        }
        if host.index_store.is_some()
            || loaded
                ._bundle
                .as_ref()
                .is_some_and(|b| b.semantic_index_bytes().is_some())
        {
            loaded.observation.semantic_index =
                unavailable(CatalogComponentUnavailable::FeatureDisabled);
        } else {
            loaded.observation.semantic_index = unavailable(CatalogComponentUnavailable::Missing);
        }
        Ok(())
    }
    #[cfg(feature = "local")]
    fn load_semantics(
        &self,
        host: &HostCatalogConfig,
        loaded: &mut LoadedCatalog,
        control: &dyn InspectionControl,
    ) -> Result<(), ProjectError> {
        use CatalogComponentUnavailable as U;
        if let Some(path) = &host.model_dir {
            match crate::catalog_semantic::load_model(path) {
                Ok(model) => {
                    loaded.observation.model = Component::Available {
                        value: model.identity().clone(),
                    };
                    loaded._model = Some(model);
                }
                Err(error) => loaded.observation.model = unavailable(semantic_error(error)),
            }
        }
        control.check()?;
        let configured = host.index_store.is_some()
            || loaded
                ._bundle
                .as_ref()
                .is_some_and(|b| b.semantic_index_bytes().is_some());
        if !configured {
            loaded.observation.semantic_index = unavailable(U::Missing);
            return Ok(());
        }
        let (Some(bundle), Some(model)) = (&loaded._bundle, &loaded._model) else {
            loaded.observation.semantic_index = unavailable(U::DependencyUnavailable);
            return Ok(());
        };
        let result = (|| {
            let external;
            let bytes = if let Some(path) = &host.index_store {
                external = catalog_store::read_private_optional_file(
                    &path.join("active.bundle"),
                    16 * 1024 * 1024,
                )
                .map_err(store_error)?
                .ok_or(U::Missing)?;
                external.as_slice()
            } else {
                bundle.semantic_index_bytes().ok_or(U::Missing)?
            };
            let metadata = IndexMetadata {
                schema_version: 1,
                snapshot_fingerprint: bundle.repository().metadata().fingerprint.clone(),
                model: model.identity().clone(),
            };
            let index = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| U::IoUnavailable)?
                .block_on(rust_engineering_semantic::LanceMemoryIndex::restore(
                    bytes,
                    metadata.clone(),
                ))
                .map_err(semantic_error)?;
            let mut names: Vec<_> = bundle
                .repository()
                .embedding_documents()
                .map_err(|_| U::Invalid)?
                .into_iter()
                .map(|(name, _)| name)
                .collect();
            names.sort();
            if names != index.crate_names() {
                return Err(U::IdentityMismatch);
            }
            Ok((
                index,
                CatalogIndexObservation {
                    metadata,
                    documents: names.len().try_into().map_err(|_| U::Budget)?,
                },
            ))
        })();
        control.check()?;
        match result {
            Ok((index, value)) => {
                loaded._index = Some(index);
                loaded.observation.semantic_index = Component::Available { value };
            }
            Err(reason) => loaded.observation.semantic_index = unavailable(reason),
        }
        Ok(())
    }
    fn rustsec(
        &self,
        control: &dyn InspectionControl,
    ) -> Result<Component<CatalogRustsecObservation>, ProjectError> {
        use CatalogComponentUnavailable as U;
        let Some(config) = &self.audit else {
            return Ok(unavailable(U::NotConfigured));
        };
        control.check()?;
        let bytes = match rust_engineering_project::read_host_snapshot(&config.path, control) {
            Ok(bytes) => bytes,
            Err(ProjectError::Cancelled) => return Err(ProjectError::Cancelled),
            Err(ProjectError::Rejected(OperationalErrorCode::CommandTimeout)) => {
                return Err(ProjectError::Rejected(OperationalErrorCode::CommandTimeout));
            }
            Err(ProjectError::Internal) => return Err(ProjectError::Internal),
            Err(ProjectError::Rejected(code)) => {
                return Ok(unavailable(match code {
                    OperationalErrorCode::UnsupportedPlatform => U::UnsupportedPlatform,
                    OperationalErrorCode::SandboxDenied | OperationalErrorCode::NetworkDenied => {
                        U::Denied
                    }
                    OperationalErrorCode::OutputLimitExceeded => U::Budget,
                    OperationalErrorCode::ProjectNotFound
                    | OperationalErrorCode::ToolNotInstalled => U::Missing,
                    _ => U::Invalid,
                }));
            }
        };
        let parsed = RustSecSnapshot::from_bytes(&bytes, &config.fingerprint, control);
        control.check()?;
        match parsed {
            Ok(snapshot) => {
                let metadata = snapshot.catalog_metadata();
                Ok(Component::Available {
                    value: CatalogRustsecObservation {
                        fingerprint: metadata.fingerprint,
                        sequence: metadata.sequence,
                        provenance: metadata.provenance,
                        record_count: snapshot.record_count(),
                    },
                })
            }
            Err(AuditDataError::Cancelled) => Err(ProjectError::Cancelled),
            Err(AuditDataError::Timeout) => {
                Err(ProjectError::Rejected(OperationalErrorCode::CommandTimeout))
            }
            Err(AuditDataError::Internal) => Err(ProjectError::Internal),
            Err(error) => Ok(unavailable(match error {
                AuditDataError::Unavailable => U::Missing,
                AuditDataError::Integrity => U::IdentityMismatch,
                AuditDataError::Budget => U::Budget,
                AuditDataError::SandboxDenied => U::Denied,
                AuditDataError::UnsupportedPlatform => U::UnsupportedPlatform,
                _ => U::Invalid,
            })),
        }
    }
}
impl CatalogStatusPort for CatalogProvider {
    fn observe(
        &self,
        control: &dyn InspectionControl,
    ) -> Result<CatalogContextObservation, ProjectError> {
        control.check()?;
        let mut state = self.state.lock().map_err(|_| ProjectError::Internal)?;
        if state.is_none() {
            *state = Some(self.load(control)?);
        }
        let mut observation = state
            .as_ref()
            .ok_or(ProjectError::Internal)?
            .observation
            .clone();
        drop(state);
        observation.rustsec = self.rustsec(control)?;
        control.check()?;
        Ok(observation)
    }
}
fn store_error(error: StoreError) -> CatalogComponentUnavailable {
    use CatalogComponentUnavailable as U;
    match error {
        StoreError::UnsupportedPlatform => U::UnsupportedPlatform,
        StoreError::Denied | StoreError::InvalidPath => U::Denied,
        StoreError::LimitExceeded => U::Budget,
        StoreError::Busy
        | StoreError::Changed
        | StoreError::Io
        | StoreError::DurabilityUncertain => U::IoUnavailable,
    }
}
#[cfg(feature = "local")]
fn semantic_error(error: SemanticError) -> CatalogComponentUnavailable {
    use CatalogComponentUnavailable as U;
    match error {
        SemanticError::MissingModel | SemanticError::MissingIndex => U::Missing,
        SemanticError::IdentityMismatch => U::IdentityMismatch,
        SemanticError::Budget => U::Budget,
        _ => U::Invalid,
    }
}

#[cfg(test)]
mod tests;
