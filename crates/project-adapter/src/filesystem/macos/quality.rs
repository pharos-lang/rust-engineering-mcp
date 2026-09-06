//! ADR-061 native APFS quality artifact store.
//!
//! The store owns one fixed private sibling of the host state root:
//!
//! ```text
//! <state-root>/rust-mcp-quality-artifacts-v1/
//!   store.lock  clock-watermark.json
//!   reservation/  blob/  descriptor/  quarantine/
//! ```
//!
//! Every host filename is generated from a canonical ASCII locator: no guest
//! filename, archive member, MIME string, URI component or tool input can ever
//! become one. The sibling `rust-mcp-mutations-v1` is never opened, traversed,
//! listed or unlinked from here.
//!
//! ## Accounting rule
//!
//! A job is charged its declared `reserved_bytes` for as long as its
//! `reservation/job_<32hex>.reserve` record exists, no matter how much has
//! actually been published; published descriptors of such a job are therefore
//! not charged a second time. Only `release` (or the expiry of the record)
//! drops the job to its actual published bytes.
//!
//! One reclamation routine (`reclaim`) runs under `store.lock` in `reconcile`,
//! at the start of every `reserve` and in `prune_expired`. It removes expired
//! descriptor/blob pairs, expired or absent reservation records with their
//! temporaries, and stale truncation markers, so accounting charges only what
//! is still on disk and on-disk bytes cannot outlive the declared caps within
//! a session. Nothing live or unexpired is ever evicted.
//!
//! An `ArchiveBundle` member is one descriptor here, but ADR-061 bounds
//! `members/job` *including archive entries*. This store cannot see inside an
//! opaque tar member, so the egress/wiring layer charges the entry count that
//! `revalidate_quality_archive` reports (`ArchiveBundleStats { entries }`)
//! against the job's declared member budget before publishing the member.
//! That charge is the integrator's obligation, not this module's.
//!
//! Because Apple's `fallocate`
//! extends the file, a member blob keeps its preallocated size until the
//! descriptor has been renamed **and** the descriptor directory fsynced; only
//! then is the surplus truncated away. Truncation therefore never runs before
//! the commit marker is durable and cannot weaken publication protection, and
//! the small descriptor write is protected by `QUALITY_CONTROL_HEADROOM_BYTES`
//! rather than by the blob's surplus blocks. An interrupted truncation is
//! completed by reconciliation **only** when the `<artifact>.trunc` marker this
//! store wrote before publication is still present; any other surplus is
//! quarantined as a size mismatch. A blob larger than its descriptor's
//! `size_bytes` never serves or hashes the surplus.
use super::state_primitives as prim;
use rust_engineering_application::{
    QUALITY_CURSOR_MAX_BYTES, QUALITY_INDEX_PAGE_MEMBERS, QUALITY_RESOURCE_CHUNK_BYTES,
    QualityArtifactChunk, QualityArtifactIndexPage, QualityArtifactInput, QualityArtifactStore,
    QualityClockSource, QualityFaultInjection, QualityFaultPoint, QualityIngest, QualityOwnerFacts,
    QualityReservation,
};
use rust_engineering_domain::{
    PruneReport, QUALITY_MAX_ARTIFACT_BYTES, QUALITY_MAX_GLOBAL_BYTES, QUALITY_MAX_JOB_BYTES,
    QUALITY_MAX_JOB_MEMBERS, QUALITY_MAX_OWNER_BYTES, QualityArtifactDescriptor,
    QualityArtifactError, QualityArtifactId, QualityClockWatermark, QualityJobId, QuarantineReason,
    RecoveryReport, UtcInstant, reservation_fits,
};
use rustix::fs::{FlockOperation, flock, ftruncate};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::OwnedFd;
use std::path::Path;
use std::str::FromStr;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const ROOT: &str = "rust-mcp-quality-artifacts-v1";
const LOCK: &str = "store.lock";
const CLOCK: &str = "clock-watermark.json";
const CLOCK_TEMP: &str = "clock-watermark.tmp";
const RESERVATION: &str = "reservation";
const BLOB: &str = "blob";
const DESCRIPTOR: &str = "descriptor";
const QUARANTINE: &str = "quarantine";

const MAX_DESCRIPTOR_BYTES: u64 = 16 * 1024;
const MAX_RECORD_BYTES: u64 = 4 * 1024;
const STREAM_BUFFER_BYTES: usize = 64 * 1024;
/// `m`, ten decimal digits, `_` and one canonical artifact locator: the whole
/// ordering key, canonical, opaque and far below 128 bytes.
const CURSOR_BYTES: usize = 49;

fn io<T>() -> Result<T, QualityArtifactError> {
    Err(QualityArtifactError::Io)
}
fn not_found<T>() -> Result<T, QualityArtifactError> {
    Err(QualityArtifactError::NotFound)
}

fn reserve_name(job: &QualityJobId) -> String {
    format!("{}.reserve", job.as_str())
}
fn part_name(job: &QualityJobId) -> String {
    format!("{}.part", job.as_str())
}
fn descriptor_temp_name(job: &QualityJobId) -> String {
    format!("{}.dtmp", job.as_str())
}
fn record_temp_name(job: &QualityJobId) -> String {
    format!("{}.rtmp", job.as_str())
}
fn truncation_name(id: &QualityArtifactId) -> String {
    format!("{}.trunc", id.as_str())
}
fn blob_name(id: &QualityArtifactId) -> String {
    format!("{}.blob", id.as_str())
}
fn descriptor_name(id: &QualityArtifactId) -> String {
    format!("{}.json", id.as_str())
}
fn parse_stem<T: FromStr>(name: &str, suffix: &str) -> Option<T> {
    name.strip_suffix(suffix).and_then(|stem| stem.parse().ok())
}

fn encode_cursor(member_index: u16, artifact_id: &QualityArtifactId) -> Vec<u8> {
    format!("m{member_index:010}_{artifact_id}").into_bytes()
}

/// The only accepted cursor spelling; anything else is the uniform not-found.
fn parse_cursor(value: &[u8]) -> Result<(u16, QualityArtifactId), QualityArtifactError> {
    let text = std::str::from_utf8(value).map_err(|_| QualityArtifactError::NotFound)?;
    let (digits, artifact) = text
        .strip_prefix('m')
        .and_then(|rest| rest.split_once('_'))
        .ok_or(QualityArtifactError::NotFound)?;
    if value.len() != CURSOR_BYTES
        || digits.len() != 10
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
    {
        return not_found();
    }
    Ok((
        digits.parse().map_err(|_| QualityArtifactError::NotFound)?,
        artifact
            .parse()
            .map_err(|_| QualityArtifactError::NotFound)?,
    ))
}

/// The durable claim on a job's bytes. It is never read as authority to serve.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReservationRecord {
    format_version: u8,
    job_id: QualityJobId,
    owner_binding: [u8; 32],
    reserved_bytes: u64,
    declared_members: u16,
    expires_at_utc: UtcInstant,
}
impl ReservationRecord {
    fn of(reservation: &QualityReservation) -> Self {
        Self {
            format_version: 1,
            job_id: reservation.job_id.clone(),
            owner_binding: reservation.owner_binding,
            reserved_bytes: reservation.reserved_bytes,
            declared_members: reservation.declared_members,
            expires_at_utc: reservation.expires_at_utc.clone(),
        }
    }
    fn validate(&self) -> Result<(), QualityArtifactError> {
        if self.format_version != 1
            || self.reserved_bytes == 0
            || self.reserved_bytes > QUALITY_MAX_JOB_BYTES
            || self.declared_members == 0
            || self.declared_members > QUALITY_MAX_JOB_MEMBERS
        {
            return Err(QualityArtifactError::InvalidLimit);
        }
        Ok(())
    }
}

/// A truncation this store decided on *before* it published a commit marker.
///
/// It is the only evidence that surplus bytes past `size_bytes` are this
/// store's own preallocation rather than a foreign append, so a long blob
/// without a matching marker is quarantined instead of repaired.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TruncationMarker {
    format_version: u8,
    artifact_id: QualityArtifactId,
    size_bytes: u64,
    capacity_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QuarantineNote {
    format_version: u8,
    reason: QuarantineReason,
}

struct Directories {
    reservation: OwnedFd,
    blob: OwnedFd,
    descriptor: OwnedFd,
    quarantine: OwnedFd,
}

/// One member stream already on disk and fsynced, not yet committed.
struct PendingMember {
    job_id: QualityJobId,
    sha256: [u8; 32],
    size_bytes: u64,
    capacity_bytes: u64,
}

#[derive(Default)]
struct Accounting {
    global_bytes: u64,
    owner_bytes: BTreeMap<[u8; 32], u64>,
}
impl Accounting {
    fn charge(&mut self, owner: [u8; 32], bytes: u64) -> Result<(), QualityArtifactError> {
        self.global_bytes = self
            .global_bytes
            .checked_add(bytes)
            .ok_or(QualityArtifactError::QuotaExceeded)?;
        let entry = self.owner_bytes.entry(owner).or_default();
        *entry = entry
            .checked_add(bytes)
            .ok_or(QualityArtifactError::QuotaExceeded)?;
        Ok(())
    }
    fn owner(&self, owner: [u8; 32]) -> u64 {
        self.owner_bytes.get(&owner).copied().unwrap_or_default()
    }
}

pub struct NativeQualityArtifactStore {
    root: OwnedFd,
    directories: Directories,
    state_root: prim::Node,
    uid: u32,
    origin_wall_seconds: u64,
    origin_monotonic: Instant,
    /// Set only by a scoped fail-closed condition; M1, M2 and every existing
    /// tool keep working while this store refuses.
    blocked: Option<QualityArtifactError>,
    pending: Option<PendingMember>,
    faults: Option<Box<dyn QualityFaultInjection>>,
    /// Set only by a test; `None` reads the host wall clock.
    clock: Option<Box<dyn QualityClockSource>>,
}

impl NativeQualityArtifactStore {
    /// Physical host facts for the ADR-060 owner-binding authority adapter.
    /// They come from the already-open no-follow state-root capability.
    pub fn state_root_identity(&self) -> ((i64, u64), u32) {
        ((self.state_root.device, self.state_root.inode), self.uid)
    }

    pub fn open(state_root: &Path) -> Result<Self, QualityArtifactError> {
        let mut store = Self::attach(state_root)?;
        let report = store.reconcile(false)?;
        if report.clock_regression {
            store.blocked = Some(QualityArtifactError::RecoveryRequired);
        }
        Ok(store)
    }

    /// Opens the store **without** taking `store.lock` and without reconciling.
    ///
    /// Reads take no lock, so a reader built this way is complete and never
    /// reports `Busy` because another session is publishing. Every publication
    /// path still takes the lock; only reconciliation is skipped, so a caller
    /// that also needs recovery uses `open` (or `reconcile_recover`) instead.
    /// Operator recovery attaches too: it must reach a store failing closed.
    pub fn attach(state_root: &Path) -> Result<Self, QualityArtifactError> {
        let child = prim::fixed_child(state_root, ROOT)?;
        let directories = Directories {
            reservation: prim::open_or_create_directory(&child.directory, RESERVATION)?,
            blob: prim::open_or_create_directory(&child.directory, BLOB)?,
            descriptor: prim::open_or_create_directory(&child.directory, DESCRIPTOR)?,
            quarantine: prim::open_or_create_directory(&child.directory, QUARANTINE)?,
        };
        prim::open_or_create_private(&child.directory, LOCK, 0)?;
        prim::open_or_create_private(&child.directory, CLOCK, MAX_RECORD_BYTES)?;
        prim::durable(&child.directory)?;
        Ok(Self {
            root: child.directory,
            directories,
            state_root: child.parent,
            uid: rustix::process::geteuid().as_raw(),
            origin_wall_seconds: wall_seconds()?,
            origin_monotonic: Instant::now(),
            blocked: None,
            pending: None,
            faults: None,
            clock: None,
        })
    }

    /// Test-only crash and ENOSPC simulation. Production installs no hook; the
    /// store behaves identically to the real failure at the same instant.
    #[doc(hidden)]
    pub fn with_fault_injection(mut self, faults: Box<dyn QualityFaultInjection>) -> Self {
        self.faults = Some(faults);
        self
    }

    /// Test-only control of the observed wall clock. Production installs none.
    ///
    /// The monotonic origin is re-based on the installed source, so the hybrid
    /// clock keeps its meaning — the later of the observed reading and the
    /// projection from session start — instead of being pinned to the host
    /// reading this session happened to open with.
    #[doc(hidden)]
    pub fn with_clock_source(
        mut self,
        clock: Box<dyn QualityClockSource>,
    ) -> Result<Self, QualityArtifactError> {
        self.origin_wall_seconds = clock.unix_seconds()?;
        self.origin_monotonic = Instant::now();
        self.clock = Some(clock);
        Ok(self)
    }

    fn fault(&self, point: QualityFaultPoint) -> Result<(), QualityArtifactError> {
        match &self.faults {
            Some(hook) => hook.arrive(point),
            None => Ok(()),
        }
    }

    /// The observed wall clock: the host's, or a test's if one is installed.
    fn observed_wall_seconds(&self) -> Result<u64, QualityArtifactError> {
        match &self.clock {
            Some(clock) => clock.unix_seconds(),
            None => wall_seconds(),
        }
    }

    fn available(&self) -> Result<(), QualityArtifactError> {
        match self.blocked {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Exclusive, non-blocking and never awaited: contention is a bounded busy
    /// rejection, so a contender never sees a second view of the global quota.
    fn lock(&self) -> Result<OwnedFd, QualityArtifactError> {
        let fd = prim::open_private_read(&self.root, LOCK)?.ok_or(QualityArtifactError::Io)?;
        prim::private_file(&fd, 0)?;
        flock(&fd, FlockOperation::NonBlockingLockExclusive).map_err(|error| {
            if error == rustix::io::Errno::WOULDBLOCK {
                QualityArtifactError::Busy
            } else {
                QualityArtifactError::Io
            }
        })?;
        Ok(fd)
    }

    /// Hybrid clock: the later of the observed wall clock and the monotonic
    /// projection from session start. A wall clock moved backwards can only
    /// shorten a TTL in session, never lengthen it.
    fn now(&self) -> Result<UtcInstant, QualityArtifactError> {
        let projected = self
            .origin_wall_seconds
            .saturating_add(self.origin_monotonic.elapsed().as_secs());
        UtcInstant::from_unix_seconds(self.observed_wall_seconds()?.max(projected))
    }

    /// `fstatfs` on the validated state-root handle, under `store.lock`.
    fn require_headroom(&self, requested: u64) -> Result<(), QualityArtifactError> {
        reservation_fits(prim::free_bytes(&self.root)?, requested)
    }

    fn write_private(
        &self,
        directory: &OwnedFd,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), QualityArtifactError> {
        prim::unlink(directory, name)?;
        let fd = prim::create_private_exclusive(directory, name)?;
        let mut file = File::from(fd);
        // A short write or ENOSPC here publishes nothing.
        file.write_all(bytes)
            .map_err(|_| QualityArtifactError::QuotaExceeded)?;
        prim::durable(&file)?;
        (prim::private_file(&file, MAX_DESCRIPTOR_BYTES)? == bytes.len() as u64)
            .then_some(())
            .ok_or(QualityArtifactError::Io)
    }

    fn read_record<T: for<'de> Deserialize<'de>>(
        &self,
        directory: &OwnedFd,
        name: &str,
        max: u64,
    ) -> Result<Option<T>, QualityArtifactError> {
        let Some(bytes) = prim::read_private(directory, name, max)? else {
            return Ok(None);
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| QualityArtifactError::RecoveryRequired)
    }

    fn reservation_record(
        &self,
        job: &QualityJobId,
    ) -> Result<Option<ReservationRecord>, QualityArtifactError> {
        let record: Option<ReservationRecord> = self.read_record(
            &self.directories.reservation,
            &reserve_name(job),
            MAX_RECORD_BYTES,
        )?;
        match record {
            Some(record) if &record.job_id != job => Err(QualityArtifactError::RecoveryRequired),
            Some(record) => record.validate().map(|()| Some(record)),
            None => Ok(None),
        }
    }

    fn descriptor_record(
        &self,
        id: &QualityArtifactId,
    ) -> Result<Option<QualityArtifactDescriptor>, QualityArtifactError> {
        let descriptor: Option<QualityArtifactDescriptor> = self.read_record(
            &self.directories.descriptor,
            &descriptor_name(id),
            MAX_DESCRIPTOR_BYTES,
        )?;
        match descriptor {
            Some(descriptor) if &descriptor.artifact_id != id => {
                Err(QualityArtifactError::RecoveryRequired)
            }
            Some(descriptor) => descriptor.validate().map(|()| Some(descriptor)),
            None => Ok(None),
        }
    }

    /// Every live reservation, keyed by job. Expired records charge nothing.
    ///
    /// `strict` fails closed on a record this store cannot read; the relaxed
    /// form is only used by reconciliation, which quarantines such a record in
    /// the same pass instead of guessing at its claim.
    fn live_reservations(
        &self,
        now: &UtcInstant,
        strict: bool,
    ) -> Result<BTreeMap<String, ReservationRecord>, QualityArtifactError> {
        let mut live = BTreeMap::new();
        for name in prim::list(&self.directories.reservation)? {
            let Some(job) = parse_stem::<QualityJobId>(&name, ".reserve") else {
                continue;
            };
            let record = match self.reservation_record(&job) {
                Ok(Some(record)) => record,
                Ok(None) => continue,
                Err(error) if strict => return Err(error),
                Err(_) => continue,
            };
            if now.unix_seconds() < record.expires_at_utc.unix_seconds() {
                live.insert(job.as_str().to_owned(), record);
            }
        }
        Ok(live)
    }

    /// Retained plus reserved bytes per owner and globally, under the lock.
    fn accounting(&self, now: &UtcInstant) -> Result<Accounting, QualityArtifactError> {
        let live = self.live_reservations(now, true)?;
        let mut accounting = Accounting::default();
        for record in live.values() {
            accounting.charge(record.owner_binding, record.reserved_bytes)?;
        }
        for name in prim::list(&self.directories.descriptor)? {
            let Some(id) = parse_stem::<QualityArtifactId>(&name, ".json") else {
                continue;
            };
            let Some(descriptor) = self.descriptor_record(&id)? else {
                continue;
            };
            // A job with a live record is already charged its whole envelope,
            // and expired evidence — which reclamation has already removed on
            // every path that reaches here — is never charged to an owner.
            if !live.contains_key(descriptor.job_id.as_str()) && !descriptor.is_expired(now) {
                accounting.charge(descriptor.owner_binding, descriptor.size_bytes)?;
            }
        }
        Ok(accounting)
    }

    /// Bytes, member count and member indices already committed for one job.
    fn job_committed(
        &self,
        job: &QualityJobId,
    ) -> Result<(u64, u16, Vec<u16>), QualityArtifactError> {
        let mut bytes = 0_u64;
        let mut members = 0_u16;
        let mut indices = Vec::new();
        for name in prim::list(&self.directories.descriptor)? {
            let Some(id) = parse_stem::<QualityArtifactId>(&name, ".json") else {
                continue;
            };
            let Some(descriptor) = self.descriptor_record(&id)? else {
                continue;
            };
            if &descriptor.job_id == job {
                bytes = bytes
                    .checked_add(descriptor.size_bytes)
                    .ok_or(QualityArtifactError::QuotaExceeded)?;
                members = members
                    .checked_add(1)
                    .ok_or(QualityArtifactError::QuotaExceeded)?;
                indices.push(descriptor.member_index);
            }
        }
        Ok((bytes, members, indices))
    }

    fn discard_job_temporaries(&self, job: &QualityJobId) -> Result<(), QualityArtifactError> {
        for name in [
            part_name(job),
            descriptor_temp_name(job),
            record_temp_name(job),
        ] {
            prim::unlink(&self.directories.reservation, &name)?;
        }
        prim::durable(&self.directories.reservation)
    }

    /// The single reclamation pass, always run under `store.lock`.
    ///
    /// It removes exactly what has stopped being charged: expired
    /// descriptor/blob pairs, expired or absent reservation records with their
    /// `.part`/`.dtmp`/`.rtmp` temporaries, and truncation markers whose
    /// descriptor no longer exists. Accounting therefore charges only what is
    /// still on disk, and on-disk bytes cannot outlive the declared caps within
    /// a session. Live claims and unexpired evidence are never displaced.
    fn reclaim(&self, now: &UtcInstant) -> Result<Reclaimed, QualityArtifactError> {
        let mut reclaimed = Reclaimed::default();
        for name in prim::list(&self.directories.descriptor)? {
            let Some(id) = parse_stem::<QualityArtifactId>(&name, ".json") else {
                continue;
            };
            let Ok(Some(descriptor)) = self.descriptor_record(&id) else {
                continue;
            };
            if descriptor.is_expired(now) {
                self.remove_pair(&id)?;
                reclaimed.pairs = reclaimed.pairs.saturating_add(1);
                reclaimed.bytes = reclaimed.bytes.saturating_add(descriptor.size_bytes);
            } else {
                reclaimed.retained = reclaimed.retained.saturating_add(1);
            }
        }
        let live = self.live_reservations(now, false)?;
        for name in prim::list(&self.directories.reservation)? {
            if let Some(job) = parse_stem::<QualityJobId>(&name, ".reserve") {
                match self.reservation_record(&job) {
                    // A record this store cannot read is evidence, not garbage:
                    // reconciliation quarantines it instead of unlinking it.
                    Err(_) => {}
                    Ok(_) if live.contains_key(job.as_str()) => {}
                    Ok(_) => {
                        prim::unlink(&self.directories.reservation, &name)?;
                        self.discard_job_temporaries(&job)?;
                        reclaimed.reservations = reclaimed.reservations.saturating_add(1);
                    }
                }
                continue;
            }
            // A temporary of a live job may belong to another session that owns
            // the reservation and holds no lock right now.
            if let Some(job) = [".part", ".dtmp", ".rtmp"]
                .into_iter()
                .find_map(|suffix| parse_stem::<QualityJobId>(&name, suffix))
            {
                if !live.contains_key(job.as_str()) {
                    prim::unlink(&self.directories.reservation, &name)?;
                    reclaimed.temporaries = reclaimed.temporaries.saturating_add(1);
                }
                continue;
            }
            if let Some(id) = parse_stem::<QualityArtifactId>(&name, ".trunc")
                && !matches!(self.descriptor_record(&id), Ok(Some(_)))
            {
                prim::unlink(&self.directories.reservation, &name)?;
                reclaimed.temporaries = reclaimed.temporaries.saturating_add(1);
            }
        }
        prim::durable(&self.directories.reservation)?;
        Ok(reclaimed)
    }

    fn quarantine(
        &self,
        directory: &OwnedFd,
        observed: &str,
        reason: QuarantineReason,
    ) -> Result<(), QualityArtifactError> {
        let mut entropy = [0_u8; 16];
        getrandom::fill(&mut entropy).map_err(|_| QualityArtifactError::Io)?;
        let target = format!("q_{:032x}.bin", u128::from_le_bytes(entropy));
        let note = format!("{target}.note");
        let bytes = serde_json::to_vec(&QuarantineNote {
            format_version: 1,
            reason,
        })
        .map_err(|_| QualityArtifactError::Io)?;
        self.write_private(&self.directories.quarantine, &note, &bytes)?;
        prim::rename_observed(directory, observed, &self.directories.quarantine, &target)?;
        prim::durable(&self.directories.quarantine)?;
        prim::durable(directory)
    }

    /// Reconciliation. Only a fully validated v1 pair is trusted; only this
    /// store's own recognizably named uncommitted temporaries are discarded;
    /// everything else is quarantined with a closed reason. It never opens,
    /// lists or removes anything outside this sibling directory.
    fn reconcile(&mut self, operator: bool) -> Result<RecoveryReport, QualityArtifactError> {
        let _lock = self.lock()?;
        let mut report = RecoveryReport::default();
        let wall = self.observed_wall_seconds()?;
        let watermark = self.read_watermark()?;
        if watermark
            .as_ref()
            .is_some_and(|watermark| watermark.observed_at_utc.unix_seconds() > wall)
        {
            report.clock_regression = true;
            if !operator {
                return Ok(report);
            }
        }
        // The operator explicitly re-bases the store on the observed clock.
        let advanced = if report.clock_regression {
            wall
        } else {
            watermark
                .map(|value| value.observed_at_utc.unix_seconds().max(wall))
                .unwrap_or(wall)
        };
        self.advance_watermark(advanced)?;
        let now = UtcInstant::from_unix_seconds(advanced)?;

        // One reclamation routine, shared with `reserve` and `prune_expired`.
        let reclaimed = self.reclaim(&now)?;
        report.released_reservations = report
            .released_reservations
            .saturating_add(reclaimed.reservations);
        report.discarded_uncommitted = report
            .discarded_uncommitted
            .saturating_add(reclaimed.temporaries);

        let live = self.live_reservations(&now, false)?;
        for name in prim::list(&self.directories.reservation)? {
            if let Some(job) = parse_stem::<QualityJobId>(&name, ".reserve") {
                // Reclamation kept only live, readable records; anything left
                // that this store cannot read is quarantined, never guessed at.
                match self.reservation_record(&job) {
                    Ok(_) if live.contains_key(job.as_str()) => {}
                    Ok(_) => {
                        prim::unlink(&self.directories.reservation, &name)?;
                        self.discard_job_temporaries(&job)?;
                        report.released_reservations =
                            report.released_reservations.saturating_add(1);
                    }
                    Err(_) => {
                        self.quarantine(
                            &self.directories.reservation,
                            &name,
                            QuarantineReason::MalformedDescriptor,
                        )?;
                        report.quarantined = report.quarantined.saturating_add(1);
                    }
                }
                continue;
            }
            let temporary = [".part", ".dtmp", ".rtmp"]
                .into_iter()
                .find_map(|suffix| parse_stem::<QualityJobId>(&name, suffix));
            match temporary {
                // A temporary of a live job may belong to another process that
                // holds no lock right now but owns the reservation.
                Some(job) if live.contains_key(job.as_str()) => continue,
                Some(_) => {
                    prim::unlink(&self.directories.reservation, &name)?;
                    report.discarded_uncommitted = report.discarded_uncommitted.saturating_add(1);
                    continue;
                }
                None => {}
            }
            // A truncation marker whose descriptor is still committed records a
            // truncation this store may complete; reclamation dropped the rest.
            if parse_stem::<QualityArtifactId>(&name, ".trunc").is_some() {
                continue;
            }
            self.quarantine(
                &self.directories.reservation,
                &name,
                QuarantineReason::UnknownName,
            )?;
            report.quarantined = report.quarantined.saturating_add(1);
        }

        let descriptors = prim::list(&self.directories.descriptor)?;
        let committed: Vec<QualityArtifactId> = descriptors
            .iter()
            .filter_map(|name| parse_stem::<QualityArtifactId>(name, ".json"))
            .collect();
        let blobs = prim::list(&self.directories.blob)?;
        for name in &blobs {
            match parse_stem::<QualityArtifactId>(name, ".blob") {
                // A blob without its commit marker was never published and is
                // never served; discarding it releases only uncommitted bytes.
                Some(id) if !committed.contains(&id) => {
                    prim::unlink(&self.directories.blob, name)?;
                    report.discarded_uncommitted = report.discarded_uncommitted.saturating_add(1);
                }
                Some(_) => {}
                None => {
                    self.quarantine(&self.directories.blob, name, QuarantineReason::UnknownName)?;
                    report.quarantined = report.quarantined.saturating_add(1);
                }
            }
        }
        for name in descriptors {
            let Some(id) = parse_stem::<QualityArtifactId>(&name, ".json") else {
                self.quarantine(
                    &self.directories.descriptor,
                    &name,
                    QuarantineReason::UnknownName,
                )?;
                report.quarantined = report.quarantined.saturating_add(1);
                continue;
            };
            match self.verify_pair(&id, &now)? {
                Verified::Valid { truncated } => {
                    report.validated = report.validated.saturating_add(1);
                    if truncated {
                        report.truncated_surplus = report.truncated_surplus.saturating_add(1);
                    }
                }
                Verified::Rejected(reason) => {
                    // Neither half is guessed at, overwritten or served again.
                    // Presence comes from the listing, so a planted symbolic
                    // link or non-regular blob is moved out too, not left.
                    let blob = blob_name(&id);
                    if blobs.contains(&blob) {
                        self.quarantine(&self.directories.blob, &blob, reason)?;
                    }
                    self.quarantine(&self.directories.descriptor, &name, reason)?;
                    report.quarantined = report.quarantined.saturating_add(1);
                }
            }
        }
        Ok(report)
    }

    fn read_watermark(&self) -> Result<Option<QualityClockWatermark>, QualityArtifactError> {
        let watermark: Option<QualityClockWatermark> =
            match prim::read_private(&self.root, CLOCK, MAX_RECORD_BYTES)? {
                Some(bytes) if bytes.is_empty() => None,
                Some(bytes) => Some(
                    serde_json::from_slice(&bytes)
                        .map_err(|_| QualityArtifactError::RecoveryRequired)?,
                ),
                None => None,
            };
        if let Some(watermark) = &watermark {
            watermark.validate()?;
        }
        Ok(watermark)
    }

    /// Whether the durable watermark is ahead of the observed wall clock.
    /// It takes no lock and advances nothing: only `recover` re-bases a clock.
    fn clock_regressed(&self) -> Result<bool, QualityArtifactError> {
        let wall = self.observed_wall_seconds()?;
        Ok(self
            .read_watermark()?
            .is_some_and(|watermark| watermark.observed_at_utc.unix_seconds() > wall))
    }

    fn advance_watermark(&self, seconds: u64) -> Result<(), QualityArtifactError> {
        self.fault(QualityFaultPoint::WatermarkAdvance)?;
        let watermark = QualityClockWatermark::new(UtcInstant::from_unix_seconds(seconds)?);
        let bytes = serde_json::to_vec(&watermark).map_err(|_| QualityArtifactError::Io)?;
        self.write_private(&self.root, CLOCK_TEMP, &bytes)?;
        prim::rename(&self.root, CLOCK_TEMP, &self.root, CLOCK)?;
        prim::durable(&self.root)
    }

    /// Full validation of one committed pair: strict schema, private regular
    /// blob with one link, exact size and SHA-256 over exactly `size_bytes`.
    fn verify_pair(
        &self,
        id: &QualityArtifactId,
        now: &UtcInstant,
    ) -> Result<Verified, QualityArtifactError> {
        // The commit marker itself must be a private regular file: a symbolic
        // link, FIFO, device or directory planted under its name is refused by
        // the kernel or by `private_file`, and is never parsed as a descriptor.
        match prim::open_private_read(&self.directories.descriptor, &descriptor_name(id)) {
            Ok(Some(fd)) if prim::private_file(&fd, MAX_DESCRIPTOR_BYTES).is_ok() => {}
            Ok(None) => return Ok(Verified::Rejected(QuarantineReason::MissingBlob)),
            Ok(Some(_)) | Err(_) => {
                return Ok(Verified::Rejected(QuarantineReason::NotPrivateRegularFile));
            }
        }
        let descriptor = match self.descriptor_record(id) {
            Ok(Some(descriptor)) => descriptor,
            Ok(None) => return Ok(Verified::Rejected(QuarantineReason::MissingBlob)),
            Err(QualityArtifactError::InvalidKindVersion) => {
                return Ok(Verified::Rejected(QuarantineReason::UnknownVersion));
            }
            Err(_) => return Ok(Verified::Rejected(QuarantineReason::MalformedDescriptor)),
        };
        if descriptor.created_at_utc.unix_seconds() > now.unix_seconds() {
            return Ok(Verified::Rejected(QuarantineReason::ClockAnomaly));
        }
        let blob = match prim::open_private_read(&self.directories.blob, &blob_name(id)) {
            Ok(Some(fd)) => Some(fd),
            Ok(None) => None,
            // The kernel refused a non-unique or non-regular object.
            Err(_) => return Ok(Verified::Rejected(QuarantineReason::NotPrivateRegularFile)),
        };
        let Some(fd) = blob else {
            return Ok(Verified::Rejected(QuarantineReason::MissingBlob));
        };
        let Ok(size) = prim::private_file(&fd, QUALITY_MAX_ARTIFACT_BYTES) else {
            return Ok(Verified::Rejected(QuarantineReason::NotPrivateRegularFile));
        };
        if size < descriptor.size_bytes {
            return Ok(Verified::Rejected(QuarantineReason::SizeMismatch));
        }
        let mut file = File::from(fd);
        if digest_prefix(&mut file, descriptor.size_bytes)? != descriptor.sha256 {
            return Ok(Verified::Rejected(QuarantineReason::DigestMismatch));
        }
        let truncated = size > descriptor.size_bytes;
        if truncated {
            // Surplus past `size_bytes` is only this store's own preallocation
            // when the marker it recorded before publication is still present
            // and describes exactly this artifact. Anything else — including a
            // same-uid append whose prefix still hashes — is ambiguous, so the
            // pair is quarantined rather than repaired.
            let marker: Option<TruncationMarker> = self
                .read_record(
                    &self.directories.reservation,
                    &truncation_name(id),
                    MAX_RECORD_BYTES,
                )
                .unwrap_or_default();
            if !marker.is_some_and(|marker| {
                marker.format_version == 1
                    && &marker.artifact_id == id
                    && marker.size_bytes == descriptor.size_bytes
                    && marker.capacity_bytes >= size
            }) {
                return Ok(Verified::Rejected(QuarantineReason::SizeMismatch));
            }
            // Completes a truncation interrupted after the commit marker.
            let writable = prim::open_private_write(&self.directories.blob, &blob_name(id))?
                .ok_or(QualityArtifactError::Io)?;
            ftruncate(&writable, descriptor.size_bytes).map_err(|_| QualityArtifactError::Io)?;
            prim::durable(&writable)?;
            prim::durable(&self.directories.blob)?;
            prim::unlink(&self.directories.reservation, &truncation_name(id))?;
            prim::durable(&self.directories.reservation)?;
        }
        Ok(Verified::Valid { truncated })
    }

    fn remove_pair(&self, id: &QualityArtifactId) -> Result<(), QualityArtifactError> {
        prim::unlink(&self.directories.descriptor, &descriptor_name(id))?;
        prim::durable(&self.directories.descriptor)?;
        prim::unlink(&self.directories.blob, &blob_name(id))?;
        prim::durable(&self.directories.blob)
    }
}

enum Verified {
    Valid { truncated: bool },
    Rejected(QuarantineReason),
}

/// What one reclamation pass removed. Bounded counts only, never a name.
#[derive(Default)]
struct Reclaimed {
    pairs: u32,
    bytes: u64,
    retained: u32,
    reservations: u32,
    temporaries: u32,
}

fn wall_seconds() -> Result<u64, QualityArtifactError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .map_err(|_| QualityArtifactError::InvalidTimestamp)
}

fn digest_prefix(file: &mut File, length: u64) -> Result<[u8; 32], QualityArtifactError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| QualityArtifactError::Io)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; STREAM_BUFFER_BYTES];
    let mut remaining = length;
    while remaining > 0 {
        let want = usize::try_from(remaining.min(STREAM_BUFFER_BYTES as u64))
            .map_err(|_| QualityArtifactError::Io)?;
        let read = file
            .read(&mut buffer[..want])
            .map_err(|_| QualityArtifactError::Io)?;
        if read == 0 || read > want {
            return io();
        }
        hasher.update(&buffer[..read]);
        remaining -= read as u64;
    }
    Ok(hasher.finalize().into())
}

impl QualityArtifactStore for NativeQualityArtifactStore {
    /// Domain-separated SHA-256 over the state root device/inode, the host uid,
    /// the granted root device/inode and the granted workspace-root string.
    /// No peer ID, fingerprint, `ProjectRef`, artifact ID or URI text enters it.
    fn owner_binding(&self, facts: &QualityOwnerFacts) -> Result<[u8; 32], QualityArtifactError> {
        if facts.workspace_root.is_empty()
            || facts.workspace_root.len() > 4096
            || facts.workspace_root.contains('\0')
        {
            return Err(QualityArtifactError::Unauthorized);
        }
        let mut hash = Sha256::new();
        hash.update(b"rust-engineering-mcp/quality-artifact-owner-binding/v1\0");
        for field in [
            self.state_root.device.to_le_bytes(),
            self.state_root.inode.to_le_bytes(),
            u64::from(self.uid).to_le_bytes(),
            facts.granted_root_device.to_le_bytes(),
            facts.granted_root_inode.to_le_bytes(),
        ] {
            hash.update((field.len() as u64).to_le_bytes());
            hash.update(field);
        }
        hash.update((facts.workspace_root.len() as u64).to_le_bytes());
        hash.update(facts.workspace_root.as_bytes());
        Ok(hash.finalize().into())
    }

    fn reserve(&mut self, reservation: &QualityReservation) -> Result<(), QualityArtifactError> {
        self.available()?;
        reservation.validate()?;
        let _lock = self.lock()?;
        let now = self.now()?;
        if now.unix_seconds() >= reservation.expires_at_utc.unix_seconds() {
            return Err(QualityArtifactError::Expired);
        }
        // Nothing that has stopped being charged may still occupy the volume or
        // the budget when a new claim is judged.
        self.reclaim(&now)?;
        let record = ReservationRecord::of(reservation);
        if let Some(existing) = self.reservation_record(&reservation.job_id)? {
            // Re-admission of the same job is idempotent; a different claim on
            // the same locator is a conflict, never a silent second view.
            return (existing == record)
                .then_some(())
                .ok_or(QualityArtifactError::RecoveryRequired);
        }
        let accounting = self.accounting(&now)?;
        let owner_total = accounting
            .owner(reservation.owner_binding)
            .checked_add(reservation.reserved_bytes)
            .ok_or(QualityArtifactError::QuotaExceeded)?;
        let global_total = accounting
            .global_bytes
            .checked_add(reservation.reserved_bytes)
            .ok_or(QualityArtifactError::QuotaExceeded)?;
        // Saturation rejects before any gateway runs; nothing is ever evicted.
        if owner_total > QUALITY_MAX_OWNER_BYTES || global_total > QUALITY_MAX_GLOBAL_BYTES {
            return Err(QualityArtifactError::QuotaExceeded);
        }
        self.require_headroom(reservation.reserved_bytes)?;
        let bytes = serde_json::to_vec(&record).map_err(|_| QualityArtifactError::Io)?;
        let temp = record_temp_name(&reservation.job_id);
        self.write_private(&self.directories.reservation, &temp, &bytes)?;
        prim::rename(
            &self.directories.reservation,
            &temp,
            &self.directories.reservation,
            &reserve_name(&reservation.job_id),
        )?;
        prim::durable(&self.directories.reservation)
    }

    fn release(&mut self, reservation: &QualityReservation) -> Result<(), QualityArtifactError> {
        self.available()?;
        let _lock = self.lock()?;
        self.pending = None;
        // Only the holder of the exact claim may drop it: knowing a job locator
        // is not authority to release another session's record or temporaries.
        match self.reservation_record(&reservation.job_id)? {
            Some(record) if record == ReservationRecord::of(reservation) => {}
            Some(_) => return Err(QualityArtifactError::Unauthorized),
            None => return Ok(()),
        }
        prim::unlink(
            &self.directories.reservation,
            &reserve_name(&reservation.job_id),
        )?;
        self.discard_job_temporaries(&reservation.job_id)
    }

    fn ingest_member(
        &mut self,
        reservation: &QualityReservation,
        member_index: u16,
        member_cap_bytes: u64,
        input: &mut dyn QualityArtifactInput,
    ) -> Result<QualityIngest, QualityArtifactError> {
        self.available()?;
        reservation.validate()?;
        if member_index >= reservation.declared_members
            || member_cap_bytes == 0
            || member_cap_bytes > QUALITY_MAX_ARTIFACT_BYTES
            || member_cap_bytes > reservation.reserved_bytes
        {
            return Err(QualityArtifactError::InvalidLimit);
        }
        let _lock = self.lock()?;
        self.pending = None;
        let now = self.now()?;
        let record = self
            .reservation_record(&reservation.job_id)?
            .ok_or(QualityArtifactError::Unauthorized)?;
        if record != ReservationRecord::of(reservation) {
            return Err(QualityArtifactError::Unauthorized);
        }
        if now.unix_seconds() >= record.expires_at_utc.unix_seconds() {
            return Err(QualityArtifactError::Expired);
        }
        let (published_bytes, published_members, _) = self.job_committed(&reservation.job_id)?;
        if published_bytes
            .checked_add(member_cap_bytes)
            .is_none_or(|total| total > reservation.reserved_bytes)
            || published_members >= reservation.declared_members
        {
            return Err(QualityArtifactError::QuotaExceeded);
        }
        self.require_headroom(member_cap_bytes)?;

        let name = part_name(&reservation.job_id);
        prim::unlink(&self.directories.reservation, &name)?;
        let fd = prim::create_private_exclusive(&self.directories.reservation, &name)?;
        // Best effort only: APFS snapshots, purgeable space and other writers
        // can still consume it, so every write below stays fail-closed.
        let _ = rustix::fs::fallocate(
            &fd,
            rustix::fs::FallocateFlags::empty(),
            0,
            member_cap_bytes,
        );
        let mut file = File::from(fd);
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; STREAM_BUFFER_BYTES];
        let mut written = 0_u64;
        let outcome = loop {
            let read = match input.read(&mut buffer) {
                Ok(read) if read <= buffer.len() => read,
                Ok(_) => break Err(QualityArtifactError::Io),
                Err(error) => break Err(error),
            };
            if read == 0 {
                break Ok(());
            }
            match written.checked_add(read as u64) {
                Some(total) if total <= member_cap_bytes => written = total,
                // Exact byte cap: a flood stops here and publishes nothing.
                _ => break Err(QualityArtifactError::QuotaExceeded),
            }
            if let Err(error) = self.fault(QualityFaultPoint::IngestWrite) {
                break Err(error);
            }
            if file.write_all(&buffer[..read]).is_err() {
                break Err(QualityArtifactError::QuotaExceeded);
            }
            hasher.update(&buffer[..read]);
        };
        if let Err(error) = outcome.and_then(|()| prim::durable(&file)) {
            drop(file);
            // Release only this job's own known temporary; the reservation
            // record stays, so the claim is never silently widened or lost.
            let _ = prim::unlink(&self.directories.reservation, &name);
            let _ = prim::durable(&self.directories.reservation);
            return Err(error);
        }
        let size = prim::private_file(&file, member_cap_bytes.max(written))?;
        if size < written {
            return Err(QualityArtifactError::Io);
        }
        let ingest = QualityIngest {
            sha256: hasher.finalize().into(),
            size_bytes: written,
        };
        self.pending = Some(PendingMember {
            job_id: reservation.job_id.clone(),
            sha256: ingest.sha256,
            size_bytes: ingest.size_bytes,
            capacity_bytes: size,
        });
        Ok(ingest)
    }

    fn publish_descriptor(
        &mut self,
        reservation: &QualityReservation,
        descriptor: &QualityArtifactDescriptor,
    ) -> Result<(), QualityArtifactError> {
        self.available()?;
        reservation.validate()?;
        descriptor.validate()?;
        let pending = self.pending.take().ok_or(QualityArtifactError::Io)?;
        if descriptor.owner_binding != reservation.owner_binding
            || descriptor.job_id != reservation.job_id
            || pending.job_id != descriptor.job_id
            || pending.sha256 != descriptor.sha256
            || pending.size_bytes != descriptor.size_bytes
        {
            return Err(QualityArtifactError::InvalidDescriptor);
        }
        let _lock = self.lock()?;
        let now = self.now()?;
        let record = self
            .reservation_record(&reservation.job_id)?
            .ok_or(QualityArtifactError::Unauthorized)?;
        if record != ReservationRecord::of(reservation) {
            return Err(QualityArtifactError::Unauthorized);
        }
        // An already-expired descriptor is never committed.
        if now.unix_seconds() >= record.expires_at_utc.unix_seconds() || descriptor.is_expired(&now)
        {
            return Err(QualityArtifactError::Expired);
        }
        if self
            .descriptor_record(&descriptor.artifact_id)
            .is_ok_and(|existing| existing.is_some())
        {
            return Err(QualityArtifactError::RecoveryRequired);
        }
        // One member index is one member: a job may not publish two descriptors
        // at the same index, so the index totally orders a job's page.
        let (_, _, indices) = self.job_committed(&reservation.job_id)?;
        if indices.contains(&descriptor.member_index) {
            return Err(QualityArtifactError::InvalidDescriptor);
        }
        // The durable watermark is advanced before the commit marker, so a
        // crash can never leave a descriptor stamped later than the watermark.
        self.advance_watermark(now.unix_seconds())?;
        // The truncation this store is about to owe itself is recorded, durably
        // and before the blob is renamed into place, so that reconciliation can
        // tell its own surplus from a foreign append.
        if pending.capacity_bytes > descriptor.size_bytes {
            let marker = serde_json::to_vec(&TruncationMarker {
                format_version: 1,
                artifact_id: descriptor.artifact_id.clone(),
                size_bytes: descriptor.size_bytes,
                capacity_bytes: pending.capacity_bytes,
            })
            .map_err(|_| QualityArtifactError::Io)?;
            self.write_private(
                &self.directories.reservation,
                &truncation_name(&descriptor.artifact_id),
                &marker,
            )?;
            prim::durable(&self.directories.reservation)?;
        }
        let part = part_name(&reservation.job_id);
        let blob = blob_name(&descriptor.artifact_id);
        {
            let fd = prim::open_private_read(&self.directories.reservation, &part)?
                .ok_or(QualityArtifactError::Io)?;
            // Identity is revalidated immediately before the rename.
            let size = prim::private_file(&fd, pending.capacity_bytes)?;
            if size < descriptor.size_bytes {
                return Err(QualityArtifactError::Io);
            }
        }
        prim::rename(
            &self.directories.reservation,
            &part,
            &self.directories.blob,
            &blob,
        )?;
        prim::durable(&self.directories.blob)?;
        prim::durable(&self.directories.reservation)?;
        self.fault(QualityFaultPoint::AfterBlobRename)?;

        let bytes = serde_json::to_vec(descriptor).map_err(|_| QualityArtifactError::Io)?;
        if bytes.len() as u64 > MAX_DESCRIPTOR_BYTES {
            return Err(QualityArtifactError::InvalidDescriptor);
        }
        let temp = descriptor_temp_name(&reservation.job_id);
        self.write_private(&self.directories.reservation, &temp, &bytes)?;
        prim::rename(
            &self.directories.reservation,
            &temp,
            &self.directories.descriptor,
            &descriptor_name(&descriptor.artifact_id),
        )?;
        self.fault(QualityFaultPoint::AfterDescriptorRename)?;
        // The descriptor is the commit marker; it is durable from here.
        prim::durable(&self.directories.descriptor)?;
        prim::durable(&self.directories.reservation)?;

        if pending.capacity_bytes > descriptor.size_bytes {
            let fd = prim::open_private_write(&self.directories.blob, &blob)?
                .ok_or(QualityArtifactError::Io)?;
            let file = File::from(fd);
            ftruncate(&file, descriptor.size_bytes).map_err(|_| QualityArtifactError::Io)?;
            prim::durable(&file)?;
            prim::durable(&self.directories.blob)?;
            // The debt is paid; the marker must not authorize a later surplus.
            prim::unlink(
                &self.directories.reservation,
                &truncation_name(&descriptor.artifact_id),
            )?;
            prim::durable(&self.directories.reservation)?;
        }
        Ok(())
    }

    /// Serves at most one bounded chunk of one committed member.
    ///
    /// Identity — private regular file, one link, this uid, mode 0600, at least
    /// the declared size — is revalidated on every read. The stored SHA-256 is
    /// verified over the whole blob at reconciliation and by `recover`, not per
    /// chunk: rehashing 32 MiB for each 320 KiB request would make a read cost
    /// grow with the artifact, and a chunk is never presented as a digest.
    fn read_chunk(
        &mut self,
        owner_binding: [u8; 32],
        artifact_id: &QualityArtifactId,
        offset: u64,
        length: u32,
    ) -> Result<QualityArtifactChunk, QualityArtifactError> {
        self.available()?;
        if length as usize > QUALITY_RESOURCE_CHUNK_BYTES {
            return not_found();
        }
        // No lock, no write, no lease touch and no TTL renewal on the read path.
        let Ok(Some(descriptor)) = self.descriptor_record(artifact_id) else {
            return not_found();
        };
        let now = self.now().map_err(|_| QualityArtifactError::NotFound)?;
        if descriptor.owner_binding != owner_binding
            || descriptor.is_expired(&now)
            || offset > descriptor.size_bytes
        {
            return not_found();
        }
        // A blob the kernel refuses to open uniquely — a second hard link, a
        // symbolic link, a device — is the same not-found as an absent one.
        let Some(fd) = prim::open_private_read(&self.directories.blob, &blob_name(artifact_id))
            .ok()
            .flatten()
        else {
            return not_found();
        };
        let Ok(size) = prim::private_file(&fd, QUALITY_MAX_ARTIFACT_BYTES) else {
            return not_found();
        };
        if size < descriptor.size_bytes {
            return not_found();
        }
        let want = descriptor
            .size_bytes
            .saturating_sub(offset)
            .min(u64::from(length));
        let want = usize::try_from(want).map_err(|_| QualityArtifactError::NotFound)?;
        let mut file = File::from(fd);
        file.seek(SeekFrom::Start(offset))
            .map_err(|_| QualityArtifactError::NotFound)?;
        let mut bytes = vec![0_u8; want];
        // Preallocated surplus beyond `size_bytes` is never read or served.
        file.read_exact(&mut bytes)
            .map_err(|_| QualityArtifactError::NotFound)?;
        Ok(QualityArtifactChunk {
            descriptor,
            offset,
            bytes,
        })
    }

    fn read_index_page(
        &mut self,
        owner_binding: [u8; 32],
        job_id: &QualityJobId,
        cursor: Option<&[u8]>,
    ) -> Result<QualityArtifactIndexPage, QualityArtifactError> {
        self.available()?;
        let start = match cursor {
            None => None,
            Some(value) => Some(parse_cursor(value)?),
        };
        let now = self.now()?;
        let mut rows = Vec::new();
        for name in prim::list(&self.directories.descriptor)? {
            let Some(id) = parse_stem::<QualityArtifactId>(&name, ".json") else {
                continue;
            };
            let Ok(Some(descriptor)) = self.descriptor_record(&id) else {
                continue;
            };
            // The page advances on the whole ordering key, so a job whose
            // stored objects share a member index still pages forward.
            let after_cursor = start.as_ref().is_none_or(|(index, id)| {
                (descriptor.member_index, descriptor.artifact_id.as_str()) >= (*index, id.as_str())
            });
            if &descriptor.job_id == job_id
                && descriptor.owner_binding == owner_binding
                && after_cursor
                && !descriptor.is_expired(&now)
            {
                rows.push(descriptor);
            }
        }
        rows.sort_by(|left, right| {
            (left.member_index, left.artifact_id.as_str())
                .cmp(&(right.member_index, right.artifact_id.as_str()))
        });
        let next_cursor = rows
            .get(QUALITY_INDEX_PAGE_MEMBERS)
            .map(|row| encode_cursor(row.member_index, &row.artifact_id))
            .filter(|cursor| cursor.len() <= QUALITY_CURSOR_MAX_BYTES);
        rows.truncate(QUALITY_INDEX_PAGE_MEMBERS);
        Ok(QualityArtifactIndexPage { rows, next_cursor })
    }

    fn reconcile_recover(&mut self) -> Result<RecoveryReport, QualityArtifactError> {
        self.available()?;
        self.reconcile(false)
    }

    fn prune_expired(&mut self) -> Result<PruneReport, QualityArtifactError> {
        self.available()?;
        let _lock = self.lock()?;
        let now = self.now()?;
        // Expired evidence and expired claims stop being charged and stop
        // occupying the volume; live ones are never displaced or evicted.
        let reclaimed = self.reclaim(&now)?;
        Ok(PruneReport {
            removed: reclaimed.pairs.saturating_add(reclaimed.reservations),
            reclaimed_bytes: reclaimed.bytes,
            retained: reclaimed.retained,
        })
    }
}

/// Host-operator recovery: reconciles the store even when it is failing closed,
/// re-basing the durable clock watermark on the observed wall clock. It touches
/// only this sibling; M2 state and M1 artifacts are never inspected.
pub fn recover(state_root: &Path) -> Result<RecoveryReport, QualityArtifactError> {
    NativeQualityArtifactStore::attach(state_root)?.reconcile(true)
}

/// Host-operator pruning of expired quality objects only. Never an eviction:
/// live evidence inside budget is retained and nothing else is removed.
///
/// A durable clock regression makes expiry unjudgeable, so pruning fails closed
/// and the operator must run `recover` first; pruning never re-bases the clock.
pub fn prune_expired(state_root: &Path) -> Result<PruneReport, QualityArtifactError> {
    let mut store = NativeQualityArtifactStore::attach(state_root)?;
    // Reading the watermark is enough to fail closed, and it leaves the whole
    // reclamation — and therefore the operator's report — to the prune itself.
    if store.clock_regressed()? {
        return Err(QualityArtifactError::RecoveryRequired);
    }
    store.prune_expired()
}
