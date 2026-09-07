//! ADR-061 authorization, retention and egress bounds above the adapter.
use rust_engineering_application::{
    QUALITY_CURSOR_MAX_BYTES, QUALITY_INDEX_PAGE_MEMBERS, QUALITY_RESOURCE_CHUNK_BYTES,
    QualityArtifactAccess, QualityArtifactChunk, QualityArtifactIndexPage, QualityArtifactInput,
    QualityArtifactStore, QualityAuthority, QualityIngest, QualityOwnerFacts, QualityReservation,
    QualityRetentionGrant, quality_member_charge,
};

#[test]
fn an_archive_bundle_is_one_member_with_independently_validated_entries() {
    assert_eq!(
        quality_member_charge(QualityArtifactKind::JunitXml, None),
        Ok(1)
    );
    assert_eq!(
        quality_member_charge(QualityArtifactKind::ArchiveBundle, Some(1)),
        Ok(1)
    );
    assert_eq!(
        quality_member_charge(QualityArtifactKind::ArchiveBundle, Some(127)),
        Ok(1)
    );
    assert_eq!(
        quality_member_charge(QualityArtifactKind::ArchiveBundle, Some(128)),
        Ok(1)
    );
    assert_eq!(
        quality_member_charge(QualityArtifactKind::ArchiveBundle, None),
        Err(QualityArtifactError::InvalidLimit)
    );
    assert_eq!(
        quality_member_charge(QualityArtifactKind::ToolLog, Some(1)),
        Err(QualityArtifactError::InvalidLimit)
    );
}
use rust_engineering_domain::{
    ArtifactCompleteness, ArtifactPlugin, ArtifactRuntime, ArtifactSelection, ArtifactSensitivity,
    ArtifactSource, GuestArtifactName, PayloadFormatVersion, PluginIdentity, ProjectRef,
    PruneReport, QUALITY_MAX_ARTIFACT_BYTES, QualityArtifactDescriptor, QualityArtifactDraft,
    QualityArtifactError, QualityArtifactId, QualityArtifactKind, QualityJobId, QualityMimeType,
    RecoveryReport, UtcInstant,
};

type Check = Result<(), Box<dyn std::error::Error>>;
const OWNER: [u8; 32] = [7; 32];

#[derive(Default)]
struct Store {
    reserved: Option<QualityReservation>,
    published: Vec<QualityArtifactDescriptor>,
    released: usize,
    ingested: usize,
    reads: usize,
}
impl QualityArtifactStore for Store {
    fn owner_binding(&self, facts: &QualityOwnerFacts) -> Result<[u8; 32], QualityArtifactError> {
        // A distinct granted root must never derive the same binding.
        let mut binding = OWNER;
        binding[0] = binding[0].wrapping_add(facts.granted_root_inode as u8);
        Ok(binding)
    }
    fn reserve(&mut self, reservation: &QualityReservation) -> Result<(), QualityArtifactError> {
        self.reserved = Some(reservation.clone());
        Ok(())
    }
    fn release(&mut self, _: &QualityReservation) -> Result<(), QualityArtifactError> {
        self.released += 1;
        Ok(())
    }
    fn ingest_member(
        &mut self,
        _: &QualityReservation,
        _: u16,
        cap: u64,
        input: &mut dyn QualityArtifactInput,
    ) -> Result<QualityIngest, QualityArtifactError> {
        self.ingested += 1;
        let mut buffer = [0_u8; 64];
        let mut size = 0_u64;
        loop {
            let read = input.read(&mut buffer)? as u64;
            if read == 0 {
                break;
            }
            size += read;
            if size > cap {
                return Err(QualityArtifactError::QuotaExceeded);
            }
        }
        Ok(QualityIngest {
            sha256: [9; 32],
            size_bytes: size,
        })
    }
    fn publish_descriptor(
        &mut self,
        _: &QualityReservation,
        descriptor: &QualityArtifactDescriptor,
    ) -> Result<(), QualityArtifactError> {
        self.published.push(descriptor.clone());
        Ok(())
    }
    fn read_chunk(
        &mut self,
        owner_binding: [u8; 32],
        artifact_id: &QualityArtifactId,
        offset: u64,
        length: u32,
    ) -> Result<QualityArtifactChunk, QualityArtifactError> {
        self.reads += 1;
        let descriptor = self
            .published
            .iter()
            .find(|row| &row.artifact_id == artifact_id && row.owner_binding == owner_binding)
            .ok_or(QualityArtifactError::NotFound)?;
        Ok(QualityArtifactChunk {
            descriptor: descriptor.clone(),
            offset,
            bytes: vec![0; length as usize],
        })
    }
    fn read_index_page(
        &mut self,
        owner_binding: [u8; 32],
        job_id: &QualityJobId,
        _: Option<&[u8]>,
    ) -> Result<QualityArtifactIndexPage, QualityArtifactError> {
        self.reads += 1;
        Ok(QualityArtifactIndexPage {
            rows: self
                .published
                .iter()
                .filter(|row| &row.job_id == job_id && row.owner_binding == owner_binding)
                .cloned()
                .collect(),
            next_cursor: None,
        })
    }
    fn reconcile_recover(&mut self) -> Result<RecoveryReport, QualityArtifactError> {
        Ok(RecoveryReport::default())
    }
    fn prune_expired(&mut self) -> Result<PruneReport, QualityArtifactError> {
        Ok(PruneReport::default())
    }
}

struct Authority {
    inode: u64,
    live: bool,
    calls: usize,
}
impl Default for Authority {
    fn default() -> Self {
        Self {
            inode: 0,
            live: true,
            calls: 0,
        }
    }
}
impl QualityAuthority for Authority {
    fn revalidate_owner(
        &mut self,
        _: &ProjectRef,
    ) -> Result<QualityOwnerFacts, QualityArtifactError> {
        self.calls += 1;
        if !self.live {
            return Err(QualityArtifactError::Unauthorized);
        }
        Ok(QualityOwnerFacts {
            granted_root_device: 1,
            granted_root_inode: self.inode,
            workspace_root: "/private/tmp/fixture".to_owned(),
        })
    }
}

struct Bytes(Vec<u8>);
impl QualityArtifactInput for Bytes {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, QualityArtifactError> {
        let take = self.0.len().min(buffer.len());
        buffer[..take].copy_from_slice(&self.0[..take]);
        self.0.drain(..take);
        Ok(take)
    }
}

fn draft(
    member_index: u16,
    sensitivity: ArtifactSensitivity,
) -> Result<QualityArtifactDraft, QualityArtifactError> {
    let created = UtcInstant::from_unix_seconds(1_788_000_000)?;
    Ok(QualityArtifactDraft {
        artifact_id: QualityArtifactId::from_random_bytes([member_index as u8; 16]),
        member_index,
        kind: QualityArtifactKind::ToolLog,
        mime_type: QualityMimeType::TextPlain,
        payload_format_version: PayloadFormatVersion::Utf8LogV1,
        completeness: ArtifactCompleteness::Complete,
        sensitivity,
        expires_at_utc: created.checked_add_seconds(3_600)?,
        created_at_utc: created,
        source: ArtifactSource {
            captured_source_sha256: [2; 32],
            guest_name: GuestArtifactName::ToolLog,
            selection: ArtifactSelection::Workspace,
        },
        runtime: ArtifactRuntime {
            image_digest: [3; 32],
            toolchain_identity: [4; 32],
            plugin: ArtifactPlugin {
                identity: PluginIdentity::Builtin,
                version: 1,
                digest: [5; 32],
            },
            implementation_digest: [6; 32],
        },
    })
}

fn owner() -> Result<ProjectRef, QualityArtifactError> {
    "prj_0123456789abcdef0123456789abcdef"
        .parse()
        .map_err(|_| QualityArtifactError::InvalidId)
}
fn job() -> QualityJobId {
    QualityJobId::from_random_bytes([3; 16])
}
fn expires() -> Result<UtcInstant, QualityArtifactError> {
    UtcInstant::from_unix_seconds(1_788_003_600)
}

#[test]
fn publish_binds_the_job_to_a_live_grant_and_stores_measured_bytes() -> Check {
    let mut store = Store::default();
    let mut authority = Authority::default();
    let mut access = QualityArtifactAccess {
        store: &mut store,
        authority: &mut authority,
        retention: QualityRetentionGrant::Operational,
    };
    let reservation = access.begin(&owner()?, job(), 1024, 4, expires()?)?;
    let descriptor = access.publish(
        &owner()?,
        &reservation,
        draft(0, ArtifactSensitivity::Public)?,
        512,
        &mut Bytes(b"one two three".to_vec()),
    )?;
    assert_eq!(descriptor.size_bytes, 13);
    assert_eq!(descriptor.sha256, [9; 32]);
    assert_eq!(descriptor.owner_binding, reservation.owner_binding);
    access.finish(&reservation)?;
    assert_eq!(store.released, 1);
    // Admission and egress each revalidate the live grant.
    assert_eq!(authority.calls, 2);
    Ok(())
}

#[test]
fn lost_grant_or_foreign_binding_publishes_nothing() -> Check {
    let mut store = Store::default();
    let mut authority = Authority::default();
    let mut access = QualityArtifactAccess {
        store: &mut store,
        authority: &mut authority,
        retention: QualityRetentionGrant::Operational,
    };
    let reservation = access.begin(&owner()?, job(), 1024, 4, expires()?)?;
    access.authority.live = false;
    assert_eq!(
        access
            .publish(
                &owner()?,
                &reservation,
                draft(0, ArtifactSensitivity::Public)?,
                512,
                &mut Bytes(b"x".to_vec()),
            )
            .err(),
        Some(QualityArtifactError::Unauthorized)
    );
    // A different granted root derives a different binding for the same job.
    access.authority.live = true;
    access.authority.inode = 42;
    assert_eq!(
        access
            .publish(
                &owner()?,
                &reservation,
                draft(0, ArtifactSensitivity::Public)?,
                512,
                &mut Bytes(b"x".to_vec()),
            )
            .err(),
        Some(QualityArtifactError::Unauthorized)
    );
    assert!(store.published.is_empty());
    assert_eq!(store.ingested, 0);
    Ok(())
}

#[test]
fn retention_grant_gates_sensitivity_and_never_widens() -> Check {
    for (grant, sensitivity, allowed) in [
        (
            QualityRetentionGrant::Operational,
            ArtifactSensitivity::Public,
            true,
        ),
        (
            QualityRetentionGrant::Operational,
            ArtifactSensitivity::SourceDerived,
            false,
        ),
        (
            QualityRetentionGrant::SourceDerived,
            ArtifactSensitivity::SymbolDerived,
            true,
        ),
        (
            QualityRetentionGrant::SourceDerived,
            ArtifactSensitivity::PotentiallySensitive,
            false,
        ),
        (
            QualityRetentionGrant::SourceDerived,
            ArtifactSensitivity::SecretSuspected,
            false,
        ),
        (
            QualityRetentionGrant::PotentiallySensitive,
            ArtifactSensitivity::SecretSuspected,
            true,
        ),
    ] {
        let mut store = Store::default();
        let mut authority = Authority::default();
        let mut access = QualityArtifactAccess {
            store: &mut store,
            authority: &mut authority,
            retention: grant,
        };
        let reservation = access.begin(&owner()?, job(), 1024, 4, expires()?)?;
        let result = access.publish(
            &owner()?,
            &reservation,
            draft(0, sensitivity)?,
            512,
            &mut Bytes(b"x".to_vec()),
        );
        assert_eq!(result.is_ok(), allowed, "{grant:?} {sensitivity:?}");
        if !allowed {
            assert_eq!(result.err(), Some(QualityArtifactError::RetentionDenied));
            assert_eq!(store.ingested, 0, "bytes were consumed before the gate");
        }
    }
    Ok(())
}

#[test]
fn member_and_cap_limits_reject_before_any_byte_is_consumed() -> Check {
    let mut store = Store::default();
    let mut authority = Authority::default();
    let mut access = QualityArtifactAccess {
        store: &mut store,
        authority: &mut authority,
        retention: QualityRetentionGrant::Operational,
    };
    let reservation = access.begin(&owner()?, job(), 1024, 2, expires()?)?;
    assert_eq!(
        access
            .publish(
                &owner()?,
                &reservation,
                draft(2, ArtifactSensitivity::Public)?,
                512,
                &mut Bytes(b"x".to_vec())
            )
            .err(),
        Some(QualityArtifactError::InvalidDescriptor)
    );
    for cap in [0, 2048, QUALITY_MAX_ARTIFACT_BYTES + 1] {
        assert_eq!(
            access
                .publish(
                    &owner()?,
                    &reservation,
                    draft(0, ArtifactSensitivity::Public)?,
                    cap,
                    &mut Bytes(b"x".to_vec())
                )
                .err(),
            Some(QualityArtifactError::InvalidLimit)
        );
    }
    assert_eq!(store.ingested, 0);
    Ok(())
}

#[test]
fn every_malformed_read_is_the_same_not_found() -> Check {
    let mut store = Store::default();
    let mut authority = Authority::default();
    let mut access = QualityArtifactAccess {
        store: &mut store,
        authority: &mut authority,
        retention: QualityRetentionGrant::Operational,
    };
    let reservation = access.begin(&owner()?, job(), 4096, 4, expires()?)?;
    let published = access.publish(
        &owner()?,
        &reservation,
        draft(0, ArtifactSensitivity::Public)?,
        512,
        &mut Bytes(b"payload".to_vec()),
    )?;
    assert!(
        access
            .read_chunk(&owner()?, &published.artifact_id, 0, 7)
            .is_ok()
    );
    // Over-long chunk, empty cursor and over-long cursor are indistinguishable.
    assert_eq!(
        access
            .read_chunk(
                &owner()?,
                &published.artifact_id,
                0,
                QUALITY_RESOURCE_CHUNK_BYTES as u32 + 1
            )
            .err(),
        Some(QualityArtifactError::NotFound)
    );
    assert_eq!(
        access.read_index_page(&owner()?, &job(), Some(&[])).err(),
        Some(QualityArtifactError::NotFound)
    );
    assert_eq!(
        access
            .read_index_page(
                &owner()?,
                &job(),
                Some(&[b'a'; QUALITY_CURSOR_MAX_BYTES + 1])
            )
            .err(),
        Some(QualityArtifactError::NotFound)
    );
    assert!(
        access
            .read_index_page(&owner()?, &job(), Some(&[b'a'; QUALITY_CURSOR_MAX_BYTES]))
            .is_ok()
    );
    // A different granted root sees the same not-found, never an index leak.
    access.authority.inode = 42;
    assert_eq!(
        access
            .read_chunk(&owner()?, &published.artifact_id, 0, 7)
            .err(),
        Some(QualityArtifactError::NotFound)
    );
    let page = access.read_index_page(&owner()?, &job(), None)?;
    assert!(page.rows.is_empty());
    assert!(page.rows.len() <= QUALITY_INDEX_PAGE_MEMBERS);
    // A lost grant is also the same status.
    access.authority.live = false;
    assert_eq!(
        access
            .read_chunk(&owner()?, &published.artifact_id, 0, 7)
            .err(),
        Some(QualityArtifactError::NotFound)
    );
    assert_eq!(
        access.read_index_page(&owner()?, &job(), None).err(),
        Some(QualityArtifactError::NotFound)
    );
    Ok(())
}

#[test]
fn a_page_that_leaks_another_owner_or_job_is_rejected() -> Check {
    struct Leaky(QualityArtifactDescriptor);
    impl QualityArtifactStore for Leaky {
        fn owner_binding(&self, _: &QualityOwnerFacts) -> Result<[u8; 32], QualityArtifactError> {
            Ok(OWNER)
        }
        fn reserve(&mut self, _: &QualityReservation) -> Result<(), QualityArtifactError> {
            Ok(())
        }
        fn release(&mut self, _: &QualityReservation) -> Result<(), QualityArtifactError> {
            Ok(())
        }
        fn ingest_member(
            &mut self,
            _: &QualityReservation,
            _: u16,
            _: u64,
            _: &mut dyn QualityArtifactInput,
        ) -> Result<QualityIngest, QualityArtifactError> {
            Err(QualityArtifactError::Io)
        }
        fn publish_descriptor(
            &mut self,
            _: &QualityReservation,
            _: &QualityArtifactDescriptor,
        ) -> Result<(), QualityArtifactError> {
            Ok(())
        }
        fn read_chunk(
            &mut self,
            _: [u8; 32],
            _: &QualityArtifactId,
            _: u64,
            _: u32,
        ) -> Result<QualityArtifactChunk, QualityArtifactError> {
            Err(QualityArtifactError::NotFound)
        }
        fn read_index_page(
            &mut self,
            _: [u8; 32],
            _: &QualityJobId,
            _: Option<&[u8]>,
        ) -> Result<QualityArtifactIndexPage, QualityArtifactError> {
            Ok(QualityArtifactIndexPage {
                rows: vec![self.0.clone()],
                next_cursor: None,
            })
        }
        fn reconcile_recover(&mut self) -> Result<RecoveryReport, QualityArtifactError> {
            Ok(RecoveryReport::default())
        }
        fn prune_expired(&mut self) -> Result<PruneReport, QualityArtifactError> {
            Ok(PruneReport::default())
        }
    }
    let foreign = draft(0, ArtifactSensitivity::Public)?.into_descriptor(
        QualityJobId::from_random_bytes([9; 16]),
        [1; 32],
        [0; 32],
        1,
    )?;
    let mut store = Leaky(foreign);
    let mut authority = Authority::default();
    let mut access = QualityArtifactAccess {
        store: &mut store,
        authority: &mut authority,
        retention: QualityRetentionGrant::Operational,
    };
    assert_eq!(
        access.read_index_page(&owner()?, &job(), None).err(),
        Some(QualityArtifactError::NotFound)
    );
    Ok(())
}
