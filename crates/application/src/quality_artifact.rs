//! Authorization and ports for ADR-061 quality artifacts.
//!
//! This layer deliberately has no paths, clocks, runtime, archive parser or URI
//! grammar. It decides *whether* an owner may publish or read, and delegates
//! every physical effect to a store adapter that owns the state root.
use crate::ArtifactStore;
use rust_engineering_domain::{
    ArtifactSensitivity, ProjectRef, PruneReport, QUALITY_MAX_ARTIFACT_BYTES,
    QUALITY_MAX_JOB_BYTES, QUALITY_MAX_JOB_MEMBERS, QualityArtifactDescriptor,
    QualityArtifactDraft, QualityArtifactError, QualityArtifactId, QualityJobId, RecoveryReport,
    UtcInstant,
};

pub const QUALITY_RESOURCE_CHUNK_BYTES: usize = 320 * 1024;
pub const QUALITY_INDEX_PAGE_MEMBERS: usize = 64;
pub const QUALITY_CURSOR_MAX_BYTES: usize = 128;

/// Number of stored descriptors charged to the per-job member budget.
///
/// A validated archive is retained and read as one opaque member, so it is
/// charged once. Its internal entry count remains independently bounded by the
/// closed USTAR validator and never becomes a collection of Resource members.
pub fn quality_member_charge(
    kind: rust_engineering_domain::QualityArtifactKind,
    archive_entries: Option<u16>,
) -> Result<u16, QualityArtifactError> {
    let charge = match (kind, archive_entries) {
        (rust_engineering_domain::QualityArtifactKind::ArchiveBundle, Some(entries))
            if entries > 0 =>
        {
            1
        }
        (rust_engineering_domain::QualityArtifactKind::ArchiveBundle, _) => {
            return Err(QualityArtifactError::InvalidLimit);
        }
        (_, None) => 1,
        (_, Some(_)) => return Err(QualityArtifactError::InvalidLimit),
    };
    (charge <= QUALITY_MAX_JOB_MEMBERS)
        .then_some(charge)
        .ok_or(QualityArtifactError::InvalidLimit)
}

/// The physical facts a *live* registry resolution observed for a granted root.
///
/// The application never hashes them: only the store adapter, which also knows
/// its own state root and host uid, may derive an owner binding (ADR-061).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualityOwnerFacts {
    pub granted_root_device: i64,
    pub granted_root_inode: u64,
    pub workspace_root: String,
}

/// Implemented by the integrator over the live `ProjectRegistry`.
///
/// Contract: the implementation revalidates the reference against the current
/// host grant and must neither renew the idle lease nor any artifact TTL.
pub trait QualityAuthority {
    fn revalidate_owner(
        &mut self,
        project: &ProjectRef,
    ) -> Result<QualityOwnerFacts, QualityArtifactError>;
}

/// Host retention permission. Peer, client and tool input never widen it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum QualityRetentionGrant {
    /// Ordinary quality grant: only content declared free of project content.
    #[default]
    Operational,
    /// Additional host permission for source- and symbol-derived evidence.
    SourceDerived,
    /// Explicit host permission covering possibly secret-bearing evidence.
    PotentiallySensitive,
}
impl QualityRetentionGrant {
    /// A scan match proves nothing, so `SecretSuspected` needs the widest grant.
    pub fn permits(self, sensitivity: ArtifactSensitivity) -> bool {
        match self {
            Self::Operational => sensitivity == ArtifactSensitivity::Public,
            Self::SourceDerived => sensitivity <= ArtifactSensitivity::SymbolDerived,
            Self::PotentiallySensitive => true,
        }
    }
}

/// A durable, owner-bound claim on job bytes. It is not authority to read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualityReservation {
    pub job_id: QualityJobId,
    pub owner_binding: [u8; 32],
    pub reserved_bytes: u64,
    pub declared_members: u16,
    pub expires_at_utc: UtcInstant,
}
impl QualityReservation {
    pub fn validate(&self) -> Result<(), QualityArtifactError> {
        if self.reserved_bytes == 0
            || self.reserved_bytes > QUALITY_MAX_JOB_BYTES
            || self.declared_members == 0
            || self.declared_members > QUALITY_MAX_JOB_MEMBERS
        {
            return Err(QualityArtifactError::InvalidLimit);
        }
        Ok(())
    }
}

/// A bounded trusted channel supplied by a guest egress gateway. It never names a host path.
pub trait QualityArtifactInput {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, QualityArtifactError>;
}

/// What the store observed while consuming one member stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QualityIngest {
    pub sha256: [u8; 32],
    pub size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualityArtifactChunk {
    pub descriptor: QualityArtifactDescriptor,
    pub offset: u64,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualityArtifactIndexPage {
    pub rows: Vec<QualityArtifactDescriptor>,
    pub next_cursor: Option<Vec<u8>>,
}

/// Deterministic points a *test-only* hook may fail. Production installs none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualityFaultPoint {
    /// Mid-stream, after some bytes reached the reservation file.
    IngestWrite,
    /// After the blob rename, before the descriptor temp is written.
    AfterBlobRename,
    /// After the descriptor rename, before the descriptor directory fsync.
    AfterDescriptorRename,
    /// Before the durable clock watermark advance is published.
    WatermarkAdvance,
}
/// Test-only fault injection. An installed hook is a simulated crash or ENOSPC:
/// the store must fail closed at that point, exactly as the real error would.
pub trait QualityFaultInjection: Send + Sync {
    fn arrive(&self, point: QualityFaultPoint) -> Result<(), QualityArtifactError>;
}

/// Test-only source of the *observed* wall clock, in whole seconds since the
/// Unix epoch. Production installs none and reads the host clock.
///
/// It replaces only the observed reading: the store's hybrid clock still takes
/// the later of it and the monotonic projection from session start, so an
/// installed source can shorten a TTL but never lengthen one. A test that needs
/// an exact TTL boundary moves this source instead of sleeping, because a TTL
/// is expressed in whole seconds and the real margin a wall-clock test has is
/// the sub-second remainder of the instant it happened to start at.
pub trait QualityClockSource: Send + Sync {
    fn unix_seconds(&self) -> Result<u64, QualityArtifactError>;
}

/// Durable adapter boundary. Implementations must retain no authority from URI text.
pub trait QualityArtifactStore: Send {
    /// Domain-separated binding over state root, host uid and the granted root.
    fn owner_binding(&self, facts: &QualityOwnerFacts) -> Result<[u8; 32], QualityArtifactError>;
    fn reserve(&mut self, reservation: &QualityReservation) -> Result<(), QualityArtifactError>;
    /// Releases a job's remaining claim. Published descriptors are untouched.
    fn release(&mut self, reservation: &QualityReservation) -> Result<(), QualityArtifactError>;
    fn ingest_member(
        &mut self,
        reservation: &QualityReservation,
        member_index: u16,
        member_cap_bytes: u64,
        input: &mut dyn QualityArtifactInput,
    ) -> Result<QualityIngest, QualityArtifactError>;
    fn publish_descriptor(
        &mut self,
        reservation: &QualityReservation,
        descriptor: &QualityArtifactDescriptor,
    ) -> Result<(), QualityArtifactError>;
    fn read_chunk(
        &mut self,
        owner_binding: [u8; 32],
        artifact_id: &QualityArtifactId,
        offset: u64,
        length: u32,
    ) -> Result<QualityArtifactChunk, QualityArtifactError>;
    fn read_index_page(
        &mut self,
        owner_binding: [u8; 32],
        job_id: &QualityJobId,
        cursor: Option<&[u8]>,
    ) -> Result<QualityArtifactIndexPage, QualityArtifactError>;
    fn reconcile_recover(&mut self) -> Result<RecoveryReport, QualityArtifactError>;
    fn prune_expired(&mut self) -> Result<PruneReport, QualityArtifactError>;
}

/// Marker for the unchanged Stage-0 M1 fallback. Its semantics remain those of `ArtifactStore`.
pub trait QualityArtifactFallback: ArtifactStore {}
impl<T: ArtifactStore> QualityArtifactFallback for T {}

pub struct QualityArtifactAccess<'a, S: QualityArtifactStore, A: QualityAuthority> {
    pub store: &'a mut S,
    pub authority: &'a mut A,
    pub retention: QualityRetentionGrant,
}
impl<S: QualityArtifactStore, A: QualityAuthority> QualityArtifactAccess<'_, S, A> {
    fn binding(&mut self, project: &ProjectRef) -> Result<[u8; 32], QualityArtifactError> {
        let facts = self.authority.revalidate_owner(project)?;
        self.store.owner_binding(&facts)
    }

    /// Admission: bind the job to the live grant and claim its bytes up front.
    pub fn begin(
        &mut self,
        project: &ProjectRef,
        job_id: QualityJobId,
        reserved_bytes: u64,
        declared_members: u16,
        expires_at_utc: UtcInstant,
    ) -> Result<QualityReservation, QualityArtifactError> {
        let reservation = QualityReservation {
            job_id,
            owner_binding: self.binding(project)?,
            reserved_bytes,
            declared_members,
            expires_at_utc,
        };
        reservation.validate()?;
        self.store.reserve(&reservation)?;
        Ok(reservation)
    }

    /// Streams one member and commits its descriptor. Nothing is published on
    /// any failure, and the size and digest come from the store, not the caller.
    pub fn publish(
        &mut self,
        project: &ProjectRef,
        reservation: &QualityReservation,
        draft: QualityArtifactDraft,
        member_cap_bytes: u64,
        input: &mut dyn QualityArtifactInput,
    ) -> Result<QualityArtifactDescriptor, QualityArtifactError> {
        reservation.validate()?;
        if draft.member_index >= reservation.declared_members {
            return Err(QualityArtifactError::InvalidDescriptor);
        }
        if !self.retention.permits(draft.sensitivity) {
            return Err(QualityArtifactError::RetentionDenied);
        }
        if member_cap_bytes == 0
            || member_cap_bytes > QUALITY_MAX_ARTIFACT_BYTES
            || member_cap_bytes > reservation.reserved_bytes
        {
            return Err(QualityArtifactError::InvalidLimit);
        }
        // Authority is revalidated immediately before bytes are consumed; a lost
        // grant between admission and egress publishes nothing.
        if self.binding(project)? != reservation.owner_binding {
            return Err(QualityArtifactError::Unauthorized);
        }
        let ingest =
            self.store
                .ingest_member(reservation, draft.member_index, member_cap_bytes, input)?;
        let descriptor = draft.into_descriptor(
            reservation.job_id.clone(),
            reservation.owner_binding,
            ingest.sha256,
            ingest.size_bytes,
        )?;
        self.store.publish_descriptor(reservation, &descriptor)?;
        Ok(descriptor)
    }

    /// Ends the job and releases its remaining claim, keeping every descriptor.
    pub fn finish(&mut self, reservation: &QualityReservation) -> Result<(), QualityArtifactError> {
        self.store.release(reservation)
    }

    pub fn read_chunk(
        &mut self,
        project: &ProjectRef,
        artifact_id: &QualityArtifactId,
        offset: u64,
        length: u32,
    ) -> Result<QualityArtifactChunk, QualityArtifactError> {
        if usize::try_from(length).is_ok_and(|size| size > QUALITY_RESOURCE_CHUNK_BYTES) {
            return Err(QualityArtifactError::NotFound);
        }
        // Malformed, unknown, expired, revoked and mismatched all look identical.
        let binding = self
            .binding(project)
            .map_err(|_| QualityArtifactError::NotFound)?;
        let chunk = self
            .store
            .read_chunk(binding, artifact_id, offset, length)
            .map_err(|_| QualityArtifactError::NotFound)?;
        (chunk.bytes.len() <= QUALITY_RESOURCE_CHUNK_BYTES
            && chunk.descriptor.owner_binding == binding)
            .then_some(chunk)
            .ok_or(QualityArtifactError::NotFound)
    }

    pub fn read_index_page(
        &mut self,
        project: &ProjectRef,
        job_id: &QualityJobId,
        cursor: Option<&[u8]>,
    ) -> Result<QualityArtifactIndexPage, QualityArtifactError> {
        if cursor.is_some_and(|value| value.is_empty() || value.len() > QUALITY_CURSOR_MAX_BYTES) {
            return Err(QualityArtifactError::NotFound);
        }
        let binding = self
            .binding(project)
            .map_err(|_| QualityArtifactError::NotFound)?;
        let page = self
            .store
            .read_index_page(binding, job_id, cursor)
            .map_err(|_| QualityArtifactError::NotFound)?;
        if page.rows.len() > QUALITY_INDEX_PAGE_MEMBERS
            || page
                .rows
                .iter()
                .any(|row| row.owner_binding != binding || &row.job_id != job_id)
            || page
                .next_cursor
                .as_ref()
                .is_some_and(|value| value.is_empty() || value.len() > QUALITY_CURSOR_MAX_BYTES)
        {
            return Err(QualityArtifactError::NotFound);
        }
        Ok(page)
    }
}
