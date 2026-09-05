//! Closed semantic mutation orchestration over captured project and Cargo data.

use crate::{
    InspectionControl, InspectionError, ManifestEditor, MutationPublisher, ProjectError,
    ProjectInspectionPort, ProjectRegistry, ProjectSourceBackend, ReferenceGenerator,
    RegistryClock,
};
use rust_engineering_domain::{
    CargoVendorSnapshot, ManifestEdit, ManifestEditError, MutationCandidate, MutationError,
    MutationKind, MutationLockDisposition, MutationResolutionObservation,
    ProjectIdentityFingerprint, ProjectRef, SourceBundle, SourceFile, validate_source_path,
};

pub trait ProjectResolutionPort {
    fn resolve(
        &self,
        edited: &SourceBundle,
        dataset: &CargoVendorSnapshot,
        control: &dyn InspectionControl,
    ) -> Result<MutationResolutionObservation, ResolutionError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionError {
    MissingOfflineData,
    InvalidOfflineData,
    Failed,
    Inspection(InspectionError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticPreparationError {
    Mutation(MutationError),
    Edit(ManifestEditError),
    Project(ProjectError),
    Inspection(InspectionError),
    Resolution(ResolutionError),
}

impl From<MutationError> for SemanticPreparationError {
    fn from(error: MutationError) -> Self {
        Self::Mutation(error)
    }
}

impl From<ProjectError> for SemanticPreparationError {
    fn from(error: ProjectError) -> Self {
        Self::Project(error)
    }
}

fn closed_edit(kind: MutationKind, edit: &ManifestEdit, target_manifest: &str) -> bool {
    let root_patch = target_manifest == "Cargo.toml"
        && matches!(
            edit,
            ManifestEdit::LintSet { .. }
                | ManifestEdit::LintRemove { .. }
                | ManifestEdit::FeatureSet { .. }
                | ManifestEdit::FeatureRemove { .. }
                | ManifestEdit::ProfileSet { .. }
                | ManifestEdit::ProfileRemove { .. }
                | ManifestEdit::WorkspaceDependencySet { .. }
                | ManifestEdit::WorkspaceDependencyRemove { .. }
        );
    match kind {
        MutationKind::ManifestPatch => root_patch,
        MutationKind::DependencyAdd => matches!(edit, ManifestEdit::DependencyAdd { .. }),
        MutationKind::DependencyRemove => matches!(edit, ManifestEdit::DependencyRemove { .. }),
        MutationKind::FormatApply | MutationKind::FixApply => false,
    }
}

fn valid_manifest_path(path: &str) -> bool {
    validate_source_path(path).is_ok() && (path == "Cargo.toml" || path.ends_with("/Cargo.toml"))
}

impl<B: ProjectSourceBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    #[allow(clippy::too_many_arguments)] // One explicit authority boundary for semantic writes.
    pub fn prepare_semantic(
        &mut self,
        reference: &ProjectRef,
        expected_identity: &ProjectIdentityFingerprint,
        target_manifest: &str,
        kind: MutationKind,
        edit: &ManifestEdit,
        publisher: &impl MutationPublisher<B::Lease>,
        control: &dyn InspectionControl,
    ) -> Result<PreparedSemanticMutation, SemanticPreparationError> {
        if !valid_manifest_path(target_manifest) || !closed_edit(kind, edit, target_manifest) {
            return Err(MutationError::Invalid.into());
        }
        let identity = self.resolve_inner(reference, control, false)?;
        if &identity.fingerprint != expected_identity {
            return Err(MutationError::Conflict.into());
        }
        let entry = self.entries.get(reference).ok_or(MutationError::NotFound)?;
        publisher.authorize(&entry.project.lease)?;
        let before = self.source_inner(reference, control, false)?;
        if before
            .files()
            .binary_search_by(|file| file.path().cmp(target_manifest))
            .is_err()
        {
            return Err(MutationError::Invalid.into());
        }
        Ok(PreparedSemanticMutation {
            workspace_root: identity.workspace_root,
            before,
            target_manifest: target_manifest.to_owned(),
            kind,
            edit: edit.clone(),
        })
    }
}

pub struct PreparedSemanticMutation {
    workspace_root: String,
    before: SourceBundle,
    target_manifest: String,
    kind: MutationKind,
    edit: ManifestEdit,
}

impl PreparedSemanticMutation {
    pub fn validate(
        self,
        editor: &impl ManifestEditor,
        inspector: &impl ProjectInspectionPort,
        resolver: &impl ProjectResolutionPort,
        dataset: Option<&CargoVendorSnapshot>,
        control: &dyn InspectionControl,
    ) -> Result<(String, MutationCandidate), SemanticPreparationError> {
        control.check()?;
        let requires_resolution = matches!(
            &self.edit,
            ManifestEdit::FeatureSet { .. }
                | ManifestEdit::FeatureRemove { .. }
                | ManifestEdit::WorkspaceDependencySet { .. }
                | ManifestEdit::WorkspaceDependencyRemove { .. }
                | ManifestEdit::DependencyAdd { .. }
                | ManifestEdit::DependencyRemove { .. }
        );
        if matches!(
            self.kind,
            MutationKind::DependencyAdd | MutationKind::DependencyRemove
        ) {
            let structure = inspector
                .inspect(&self.before, control)
                .map_err(SemanticPreparationError::Inspection)?;
            control.check()?;
            let package = structure
                .packages
                .iter()
                .find(|package| package.manifest_path == self.target_manifest)
                .ok_or(MutationError::Invalid)?;
            if !structure.workspace_members.contains(&package.package_index) {
                return Err(MutationError::PermissionDenied.into());
            }
        }
        let dataset = if requires_resolution {
            Some(dataset.ok_or(SemanticPreparationError::Resolution(
                ResolutionError::MissingOfflineData,
            ))?)
        } else {
            None
        };

        let manifest = self
            .before
            .files()
            .iter()
            .find(|file| file.path() == self.target_manifest)
            .ok_or(MutationError::Invalid)?;
        let replacement = editor
            .apply(manifest.bytes(), &self.edit)
            .map_err(SemanticPreparationError::Edit)?;
        let files = self
            .before
            .files()
            .iter()
            .map(|file| {
                if file.path() == self.target_manifest {
                    SourceFile::new(file.path().to_owned(), replacement.clone())
                        .map_err(|_| MutationError::LimitExceeded)
                } else {
                    Ok(file.clone())
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let edited = SourceBundle::with_directories(files, self.before.directories().to_vec())
            .map_err(|_| MutationError::LimitExceeded)?;
        control.check()?;

        let (after, validation) = if requires_resolution {
            let dataset = dataset.ok_or(SemanticPreparationError::Resolution(
                ResolutionError::MissingOfflineData,
            ))?;
            let observation = resolver
                .resolve(&edited, dataset, control)
                .map_err(SemanticPreparationError::Resolution)?;
            control.check()?;
            validate_resolution_scope(
                &edited,
                &observation.candidate,
                &self.target_manifest,
                observation.lock_disposition,
            )?;
            if observation.dataset_fingerprint != dataset.tree_fingerprint {
                return Err(SemanticPreparationError::Resolution(
                    ResolutionError::InvalidOfflineData,
                ));
            }
            let version = match self.kind {
                MutationKind::ManifestPatch => "m2-manifest-resolved-v1",
                MutationKind::DependencyAdd => "m2-dependency-add-v1",
                MutationKind::DependencyRemove => "m2-dependency-remove-v1",
                MutationKind::FormatApply | MutationKind::FixApply => {
                    return Err(MutationError::Invalid.into());
                }
            };
            let disposition = match observation.lock_disposition {
                MutationLockDisposition::UpdatedExisting => "updated_existing",
                MutationLockDisposition::TransientUnpublished => "transient_unpublished",
            };
            let runtime = &observation.runtime;
            let validation = frame([
                version.to_owned(),
                "local_coordinated".to_owned(),
                runtime.platform.clone(),
                runtime.image_id.clone(),
                runtime.configuration_fingerprint.to_string(),
                runtime.execution_fingerprint.to_string(),
                runtime.rust_version.clone(),
                runtime.cargo_version.clone(),
                observation.candidate_source_fingerprint.to_string(),
                observation.resolution_execution_fingerprint.to_string(),
                observation.dataset_fingerprint.to_string(),
                observation.resolved_lock_fingerprint.to_string(),
                disposition.to_owned(),
                self.target_manifest.clone(),
            ])?;
            (observation.candidate, validation)
        } else {
            let observation = inspector
                .inspect(&edited, control)
                .map_err(SemanticPreparationError::Inspection)?;
            control.check()?;
            let runtime = &observation.runtime;
            let validation = frame([
                "m2-manifest-semantic-v1".to_owned(),
                "local_coordinated".to_owned(),
                runtime.platform.clone(),
                runtime.image_id.clone(),
                runtime.configuration_fingerprint.to_string(),
                runtime.execution_fingerprint.to_string(),
                runtime.rust_version.clone(),
                runtime.cargo_version.clone(),
                observation.source_fingerprint.to_string(),
            ])?;
            (edited, validation)
        };
        Ok((
            self.workspace_root,
            MutationCandidate {
                kind: self.kind,
                before: self.before,
                after,
                validation,
            },
        ))
    }
}

fn validate_resolution_scope(
    edited: &SourceBundle,
    candidate: &SourceBundle,
    target_manifest: &str,
    disposition: MutationLockDisposition,
) -> Result<(), SemanticPreparationError> {
    let lock_present = edited
        .files()
        .binary_search_by(|file| file.path().cmp("Cargo.lock"))
        .is_ok();
    let expected_disposition = if lock_present {
        MutationLockDisposition::UpdatedExisting
    } else {
        MutationLockDisposition::TransientUnpublished
    };
    if disposition != expected_disposition
        || edited.directories() != candidate.directories()
        || edited.files().len() != candidate.files().len()
    {
        return Err(MutationError::Invalid.into());
    }
    for (expected, actual) in edited.files().iter().zip(candidate.files()) {
        if expected.path() != actual.path()
            || (expected.bytes() != actual.bytes()
                && !(lock_present && expected.path() == "Cargo.lock"))
        {
            return Err(MutationError::PermissionDenied.into());
        }
    }
    let selected = candidate
        .files()
        .iter()
        .find(|file| file.path() == target_manifest)
        .ok_or(MutationError::Invalid)?;
    let expected = edited
        .files()
        .iter()
        .find(|file| file.path() == target_manifest)
        .ok_or(MutationError::Invalid)?;
    if selected.bytes() != expected.bytes() {
        return Err(MutationError::PermissionDenied.into());
    }
    Ok(())
}

fn frame<const N: usize>(fields: [String; N]) -> Result<String, SemanticPreparationError> {
    let mut validation = String::new();
    for field in fields {
        use std::fmt::Write;
        write!(validation, "{}:{field}", field.len()).map_err(|_| MutationError::Invalid)?;
    }
    Ok(validation)
}
