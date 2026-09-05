//! Live project authority, candidate validation and bounded preview lifetime.
use crate::{
    InspectionControl, InspectionError, ManifestEditor, OperationControl, ProjectError,
    ProjectInspectionPort, ProjectMutationPort, ProjectRegistry, ProjectSourceBackend,
    ReferenceGenerator, RegistryClock,
};
use rust_engineering_domain::{
    IdempotencyKey, ManifestEdit, ManifestEditError, MutationCandidate, MutationCommit,
    MutationError, MutationId, MutationKind, MutationReceipt, ProjectIdentityFingerprint,
    ProjectRef, RustMutationCommand, SourceBundle, SourceFile, SourceFingerprint,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// A real authority/persistence boundary. The lease is never a caller path.
pub trait MutationPublisher<L> {
    fn authorize(&self, lease: &L) -> Result<(), MutationError>;
    fn commit(
        &self,
        lease: &L,
        request: &MutationCommit,
        control: &dyn OperationControl,
    ) -> Result<MutationReceipt, MutationError>;
    fn receipt(&self, lease: &L, id: &MutationId) -> Result<MutationReceipt, MutationError>;
    fn recover(&self, lease: &L, id: &MutationId) -> Result<MutationReceipt, MutationError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutationPreparationError {
    Mutation(MutationError),
    Edit(ManifestEditError),
    Project(ProjectError),
    Inspection(InspectionError),
}
impl From<MutationError> for MutationPreparationError {
    fn from(error: MutationError) -> Self {
        Self::Mutation(error)
    }
}
impl From<ProjectError> for MutationPreparationError {
    fn from(error: ProjectError) -> Self {
        Self::Project(error)
    }
}

impl<B: ProjectSourceBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    pub fn prepare_format(
        &mut self,
        reference: &ProjectRef,
        expected_identity: &ProjectIdentityFingerprint,
        publisher: &impl MutationPublisher<B::Lease>,
        control: &dyn InspectionControl,
    ) -> Result<PreparedRustMutation, MutationPreparationError> {
        let identity = self.resolve_inner(reference, control, false)?;
        if &identity.fingerprint != expected_identity {
            return Err(MutationError::Conflict.into());
        }
        let entry = self.entries.get(reference).ok_or(MutationError::NotFound)?;
        publisher.authorize(&entry.project.lease)?;
        let before = self.source_inner(reference, control, false)?;
        Ok(PreparedRustMutation {
            workspace_root: identity.workspace_root,
            before,
        })
    }

    #[allow(clippy::too_many_arguments)] // Explicit ports at this one orchestration boundary.
    pub fn preview_manifest(
        &mut self,
        reference: &ProjectRef,
        expected_identity: &ProjectIdentityFingerprint,
        edit: &ManifestEdit,
        editor: &impl ManifestEditor,
        inspector: &impl ProjectInspectionPort,
        publisher: &impl MutationPublisher<B::Lease>,
        control: &dyn InspectionControl,
    ) -> Result<(String, MutationCandidate), MutationPreparationError> {
        let prepared = self.prepare_manifest(
            reference,
            expected_identity,
            edit,
            editor,
            publisher,
            control,
        )?;
        let candidate = prepared.validate(inspector, control)?;
        self.finish_manifest_preview(reference, &candidate.1, control)?;
        Ok(candidate)
    }

    pub fn prepare_manifest(
        &mut self,
        reference: &ProjectRef,
        expected_identity: &ProjectIdentityFingerprint,
        edit: &ManifestEdit,
        editor: &impl ManifestEditor,
        publisher: &impl MutationPublisher<B::Lease>,
        control: &dyn InspectionControl,
    ) -> Result<PreparedManifestMutation, MutationPreparationError> {
        let identity = self.resolve_inner(reference, control, false)?;
        if &identity.fingerprint != expected_identity {
            return Err(MutationError::Conflict.into());
        }
        let entry = self.entries.get(reference).ok_or(MutationError::NotFound)?;
        publisher.authorize(&entry.project.lease)?;
        let before = self.source_inner(reference, control, false)?;
        let manifest = before
            .files()
            .iter()
            .find(|file| file.path() == "Cargo.toml")
            .ok_or(MutationError::Invalid)?;
        let replacement = editor
            .apply(manifest.bytes(), edit)
            .map_err(MutationPreparationError::Edit)?;
        let files = before
            .files()
            .iter()
            .map(|file| {
                if file.path() == "Cargo.toml" {
                    SourceFile::new(file.path().into(), replacement.clone())
                        .map_err(|_| MutationError::LimitExceeded)
                } else {
                    Ok(file.clone())
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let after = SourceBundle::with_directories(files, before.directories().to_vec())
            .map_err(|_| MutationError::LimitExceeded)?;
        Ok(PreparedManifestMutation {
            workspace_root: identity.workspace_root,
            before,
            after,
        })
    }

    pub fn finish_manifest_preview(
        &mut self,
        reference: &ProjectRef,
        candidate: &MutationCandidate,
        control: &dyn InspectionControl,
    ) -> Result<(), MutationPreparationError> {
        control.check()?;
        if self.source_inner(reference, control, true)? != candidate.before {
            return Err(MutationError::Conflict.into());
        }
        Ok(())
    }

    pub fn commit_mutation(
        &mut self,
        reference: &ProjectRef,
        workspace_root: &str,
        request: &MutationCommit,
        publisher: &impl MutationPublisher<B::Lease>,
        control: &dyn OperationControl,
    ) -> Result<MutationReceipt, MutationError> {
        let identity = self
            .resolve_inner(reference, control, false)
            .map_err(project_mutation_error)?;
        if identity.workspace_root != workspace_root {
            return Err(MutationError::PermissionDenied);
        }
        let entry = self.entries.get(reference).ok_or(MutationError::NotFound)?;
        publisher.authorize(&entry.project.lease)?;
        let result = publisher.commit(&entry.project.lease, request, control);
        // Publication can invalidate the manifest identity. Never issue a fresh
        // identity under an old reference, even when the receipt was lost.
        if result.is_ok() || result == Err(MutationError::RecoveryRequired) {
            self.entries.remove(reference);
        }
        result
    }

    pub fn mutation_receipt(
        &mut self,
        reference: &ProjectRef,
        id: &MutationId,
        recover: bool,
        publisher: &impl MutationPublisher<B::Lease>,
        control: &dyn OperationControl,
    ) -> Result<MutationReceipt, MutationError> {
        self.resolve_inner(reference, control, true)
            .map_err(project_mutation_error)?;
        let entry = self.entries.get(reference).ok_or(MutationError::NotFound)?;
        publisher.authorize(&entry.project.lease)?;
        if recover {
            publisher.recover(&entry.project.lease, id)
        } else {
            publisher.receipt(&entry.project.lease, id)
        }
    }
}

/// Owned full source allows isolated mutation without retaining the registry lock.
pub struct PreparedRustMutation {
    workspace_root: String,
    before: SourceBundle,
}

impl PreparedRustMutation {
    pub fn validate(
        self,
        mutator: &impl ProjectMutationPort,
        control: &dyn InspectionControl,
    ) -> Result<(String, MutationCandidate), MutationPreparationError> {
        let observation = mutator
            .mutate(&self.before, RustMutationCommand::Format, control)
            .map_err(MutationPreparationError::Inspection)?;
        control.check()?;
        // The application independently enforces the closed operation's scope.
        // The native publisher repeats this check at the persistence boundary.
        let after = observation.candidate;
        if self.before.directories() != after.directories()
            || self.before.files().len() != after.files().len()
        {
            return Err(MutationError::Invalid.into());
        }
        let mut changed = 0;
        for (before, after) in self.before.files().iter().zip(after.files()) {
            if before.path() != after.path() {
                return Err(MutationError::Invalid.into());
            }
            if before.bytes() != after.bytes() {
                if !before.path().ends_with(".rs") {
                    return Err(MutationError::PermissionDenied.into());
                }
                changed += 1;
            }
        }
        if changed > 128 {
            return Err(MutationError::LimitExceeded.into());
        }
        let runtime = observation.runtime;
        let fields = [
            "m2-fmt-apply-v1".to_owned(),
            "local_coordinated".to_owned(),
            runtime.platform,
            runtime.image_id,
            runtime.configuration_fingerprint.to_string(),
            runtime.execution_fingerprint.to_string(),
            runtime.rust_version,
            runtime.cargo_version,
            observation.candidate_source_fingerprint.to_string(),
            observation.mutation_execution_fingerprint.to_string(),
        ];
        let mut validation = String::new();
        for field in fields {
            use std::fmt::Write;
            write!(validation, "{}:{field}", field.len()).map_err(|_| MutationError::Invalid)?;
        }
        Ok((
            self.workspace_root,
            MutationCandidate {
                kind: MutationKind::FormatApply,
                before: self.before,
                after,
                validation,
            },
        ))
    }
}

/// Captured candidate can be validated outside the shared project registry lock.
pub struct PreparedManifestMutation {
    workspace_root: String,
    before: SourceBundle,
    after: SourceBundle,
}
impl PreparedManifestMutation {
    pub fn validate(
        self,
        inspector: &impl ProjectInspectionPort,
        control: &dyn InspectionControl,
    ) -> Result<(String, MutationCandidate), MutationPreparationError> {
        let validation = inspector
            .inspect(&self.after, control)
            .map_err(MutationPreparationError::Inspection)?;
        control.check()?;
        let runtime = validation.runtime;
        let fields = [
            "m2-manifest-lints-v1".to_owned(),
            "local_coordinated".to_owned(),
            runtime.platform,
            runtime.image_id,
            runtime.configuration_fingerprint.to_string(),
            runtime.execution_fingerprint.to_string(),
            runtime.rust_version,
            runtime.cargo_version,
            validation.source_fingerprint.to_string(),
        ];
        let mut validation = String::new();
        for field in fields {
            use std::fmt::Write;
            write!(validation, "{}:{field}", field.len()).map_err(|_| MutationError::Invalid)?;
        }
        Ok((
            self.workspace_root,
            MutationCandidate {
                kind: MutationKind::ManifestPatch,
                before: self.before,
                after: self.after,
                validation,
            },
        ))
    }
}

fn project_mutation_error(error: ProjectError) -> MutationError {
    match error {
        ProjectError::Cancelled => MutationError::Cancelled,
        ProjectError::Internal => MutationError::Io,
        ProjectError::Rejected(_) => MutationError::PermissionDenied,
    }
}

pub struct RememberedMutation {
    pub workspace_root: String,
    pub request: MutationCommit,
}

struct Plan {
    id: MutationId,
    digest: SourceFingerprint,
    workspace_root: String,
    candidate: MutationCandidate,
    created: u64,
    retention: Option<PreviewToken>,
}

/// Dropping an undelivered preview revokes its budget reservation without a lock.
/// Pruning frees its owned bytes before the next admission checks capacity.
#[derive(Clone)]
pub struct PreviewToken(Arc<AtomicBool>);
pub struct PreviewRetention {
    token: PreviewToken,
    retained: bool,
}
impl Default for PreviewRetention {
    fn default() -> Self {
        Self {
            token: PreviewToken(Arc::new(AtomicBool::new(true))),
            retained: false,
        }
    }
}
impl PreviewRetention {
    pub fn token(&self) -> PreviewToken {
        self.token.clone()
    }
    pub fn retain(mut self) {
        self.retained = true;
    }
}
impl Drop for PreviewRetention {
    fn drop(&mut self) {
        if !self.retained {
            self.token.0.store(false, Ordering::Release);
        }
    }
}
impl Plan {
    fn retained(&self) -> bool {
        self.retention
            .as_ref()
            .is_none_or(|token| token.0.load(Ordering::Acquire))
    }
}

/// Preview buffers expire without source effects. Durable commits live in the publisher.
#[derive(Default)]
pub struct MutationPlans {
    entries: Vec<Plan>,
}

impl MutationPlans {
    pub const TTL_SECONDS: u64 = 600;

    pub fn remember(
        &mut self,
        id: MutationId,
        digest: SourceFingerprint,
        workspace_root: String,
        candidate: MutationCandidate,
        clock: &impl RegistryClock,
    ) -> Result<(), MutationError> {
        self.remember_inner(id, digest, workspace_root, candidate, clock, None)
    }

    #[allow(clippy::too_many_arguments)] // Exact plan binding plus delivery lifetime.
    pub fn remember_revocable(
        &mut self,
        id: MutationId,
        digest: SourceFingerprint,
        workspace_root: String,
        candidate: MutationCandidate,
        clock: &impl RegistryClock,
        retention: PreviewToken,
    ) -> Result<(), MutationError> {
        self.remember_inner(
            id,
            digest,
            workspace_root,
            candidate,
            clock,
            Some(retention),
        )
    }

    #[allow(clippy::too_many_arguments)] // Shared admission for retained and revocable previews.
    fn remember_inner(
        &mut self,
        id: MutationId,
        digest: SourceFingerprint,
        workspace_root: String,
        candidate: MutationCandidate,
        clock: &impl RegistryClock,
        retention: Option<PreviewToken>,
    ) -> Result<(), MutationError> {
        let now = clock.seconds();
        self.entries.retain(|plan| {
            plan.retained()
                && now
                    .checked_sub(plan.created)
                    .is_some_and(|age| age < Self::TTL_SECONDS)
        });
        if self.entries.iter().any(|plan| plan.id == id) {
            return Err(MutationError::Conflict);
        }
        let bytes = candidate_bytes(&candidate);
        let existing: usize = self
            .entries
            .iter()
            .map(|plan| candidate_bytes(&plan.candidate))
            .sum();
        if self.entries.len() >= 4 || existing.saturating_add(bytes) > 64 * 1024 * 1024 {
            return Err(MutationError::LimitExceeded);
        }
        self.entries.push(Plan {
            id,
            digest,
            workspace_root,
            candidate,
            created: now,
            retention,
        });
        Ok(())
    }

    pub fn resolve(
        &self,
        id: &MutationId,
        digest: &SourceFingerprint,
        key: IdempotencyKey,
        clock: &impl RegistryClock,
    ) -> Result<RememberedMutation, MutationError> {
        let plan = self
            .entries
            .iter()
            .find(|plan| &plan.id == id && plan.retained())
            .ok_or(MutationError::NotFound)?;
        if !clock
            .seconds()
            .checked_sub(plan.created)
            .is_some_and(|age| age < Self::TTL_SECONDS)
        {
            return Err(MutationError::Expired);
        }
        if &plan.digest != digest {
            return Err(MutationError::Conflict);
        }
        Ok(RememberedMutation {
            workspace_root: plan.workspace_root.clone(),
            request: MutationCommit {
                id: id.clone(),
                digest: digest.clone(),
                key,
                candidate: plan.candidate.clone(),
            },
        })
    }
}

fn candidate_bytes(candidate: &MutationCandidate) -> usize {
    candidate
        .before
        .files()
        .iter()
        .chain(candidate.after.files())
        .map(|file| file.bytes().len())
        .sum()
}
