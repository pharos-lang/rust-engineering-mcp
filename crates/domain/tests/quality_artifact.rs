//! ADR-061 descriptor, instant and capacity-floor invariants.
use rust_engineering_domain::{
    ArtifactCompleteness, ArtifactPlugin, ArtifactRuntime, ArtifactSelection, ArtifactSensitivity,
    ArtifactSource, GuestArtifactName, M2_RECOVERY_HEADROOM_BYTES, PayloadFormatVersion,
    PluginIdentity, QUALITY_CONTROL_HEADROOM_BYTES, QUALITY_MAX_ARTIFACT_BYTES,
    QUALITY_MAX_JOB_MEMBERS, QUALITY_MAX_TTL_SECONDS, QualityArtifactDescriptor,
    QualityArtifactDraft, QualityArtifactError, QualityArtifactId, QualityArtifactKind,
    QualityClockWatermark, QualityJobId, QualityMimeType, UtcInstant, reservation_fits,
};

type Check = Result<(), Box<dyn std::error::Error>>;

fn draft(created: &UtcInstant, ttl: u64) -> Result<QualityArtifactDraft, QualityArtifactError> {
    Ok(QualityArtifactDraft {
        artifact_id: QualityArtifactId::from_random_bytes([1; 16]),
        member_index: 0,
        kind: QualityArtifactKind::JunitXml,
        mime_type: QualityMimeType::ApplicationJunitXml,
        payload_format_version: PayloadFormatVersion::JunitXmlV1,
        completeness: ArtifactCompleteness::Complete,
        sensitivity: ArtifactSensitivity::Public,
        created_at_utc: created.clone(),
        expires_at_utc: created.checked_add_seconds(ttl)?,
        source: ArtifactSource {
            captured_source_sha256: [2; 32],
            guest_name: GuestArtifactName::JunitXml,
            selection: ArtifactSelection::Workspace,
        },
        runtime: ArtifactRuntime {
            image_digest: [3; 32],
            toolchain_identity: [4; 32],
            plugin: ArtifactPlugin {
                identity: PluginIdentity::Nextest,
                version: 1,
                digest: [5; 32],
            },
            implementation_digest: [6; 32],
        },
    })
}

fn descriptor(
    created: &UtcInstant,
    ttl: u64,
) -> Result<QualityArtifactDescriptor, Box<dyn std::error::Error>> {
    Ok(draft(created, ttl)?.into_descriptor(
        QualityJobId::from_random_bytes([7; 16]),
        [8; 32],
        [9; 32],
        16,
    )?)
}

#[test]
fn ids_and_kind_versions_are_closed() -> Check {
    assert!(
        "qart_0123456789abcdef0123456789abcdef"
            .parse::<QualityArtifactId>()
            .is_ok()
    );
    assert!(
        "job_0123456789abcdef0123456789abcdef"
            .parse::<QualityJobId>()
            .is_ok()
    );
    assert!(
        "qart_0123456789abcdef0123456789abcdef0"
            .parse::<QualityArtifactId>()
            .is_err()
    );
    let created = UtcInstant::from_unix_seconds(1_788_000_000)?;
    let valid = descriptor(&created, 3_600)?;
    assert_eq!(valid.validate(), Ok(()));

    // A mismatched payload version, MIME or guest name is not a v1 descriptor.
    let mut wrong = valid.clone();
    wrong.payload_format_version = PayloadFormatVersion::UstarV1;
    assert_eq!(
        wrong.validate(),
        Err(QualityArtifactError::InvalidKindVersion)
    );
    let mut wrong = valid.clone();
    wrong.mime_type = QualityMimeType::ApplicationOctetStream;
    assert_eq!(
        wrong.validate(),
        Err(QualityArtifactError::InvalidKindVersion)
    );
    let mut wrong = valid.clone();
    wrong.source.guest_name = GuestArtifactName::ToolLog;
    assert_eq!(
        wrong.validate(),
        Err(QualityArtifactError::InvalidKindVersion)
    );
    let mut wrong = valid.clone();
    wrong.format_version = 2;
    assert_eq!(
        wrong.validate(),
        Err(QualityArtifactError::InvalidDescriptor)
    );
    let mut wrong = valid.clone();
    wrong.size_bytes = QUALITY_MAX_ARTIFACT_BYTES + 1;
    assert_eq!(
        wrong.validate(),
        Err(QualityArtifactError::InvalidDescriptor)
    );
    let mut wrong = valid.clone();
    wrong.member_index = QUALITY_MAX_JOB_MEMBERS;
    assert_eq!(
        wrong.validate(),
        Err(QualityArtifactError::InvalidDescriptor)
    );
    let mut wrong = valid;
    wrong.expires_at_utc = wrong.created_at_utc.clone();
    assert_eq!(
        wrong.validate(),
        Err(QualityArtifactError::InvalidTimestamp)
    );
    assert_eq!(
        QualityArtifactError::InvalidId.to_string(),
        "invalid quality artifact identifier"
    );
    Ok(())
}

#[test]
fn descriptor_ttl_is_bounded_and_expiry_is_observational() -> Check {
    let created = UtcInstant::from_unix_seconds(1_788_000_000)?;
    assert!(
        descriptor(&created, QUALITY_MAX_TTL_SECONDS)?
            .validate()
            .is_ok()
    );
    assert_eq!(
        descriptor(&created, QUALITY_MAX_TTL_SECONDS + 1)
            .err()
            .map(|error| error.to_string()),
        Some(QualityArtifactError::InvalidTimestamp.to_string())
    );
    let artifact = descriptor(&created, 60)?;
    assert!(!artifact.is_expired(&created.checked_add_seconds(59)?));
    assert!(artifact.is_expired(&created.checked_add_seconds(60)?));
    assert!(artifact.is_expired(&created.checked_add_seconds(61)?));
    Ok(())
}

#[test]
fn size_and_digest_come_only_from_the_store() -> Check {
    // A draft cannot state its own bytes: the store supplies both.
    let created = UtcInstant::from_unix_seconds(1_788_000_000)?;
    let job = QualityJobId::from_random_bytes([7; 16]);
    let built = draft(&created, 3_600)?.into_descriptor(job.clone(), [8; 32], [9; 32], 16)?;
    assert_eq!(built.size_bytes, 16);
    assert_eq!(built.sha256, [9; 32]);
    assert_eq!(built.job_id, job);
    assert_eq!(built.owner_binding, [8; 32]);
    assert_eq!(built.format_version, 1);
    Ok(())
}

#[test]
fn instants_cover_the_calendar_and_the_range_boundaries() -> Check {
    // The 32-bit second rollover is an ordinary instant here.
    let rollover = "2038-01-19T03:14:08Z".parse::<UtcInstant>()?;
    assert_eq!(rollover.unix_seconds(), 2_147_483_648);
    assert_eq!(UtcInstant::from_unix_seconds(2_147_483_648)?, rollover);

    // The epoch is the lower bound; a second before it is not spellable.
    let epoch = UtcInstant::from_unix_seconds(0)?;
    assert_eq!(epoch.as_str(), "1970-01-01T00:00:00Z");
    assert_eq!(
        "1969-12-31T23:59:59Z".parse::<UtcInstant>().err(),
        Some(QualityArtifactError::InvalidTimestamp)
    );
    assert_eq!(epoch.seconds_until(&rollover), Some(2_147_483_648));
    assert_eq!(rollover.seconds_until(&epoch), None);

    // The upper bound round-trips and the next second leaves the range.
    let last = "9999-12-31T23:59:59Z".parse::<UtcInstant>()?;
    assert_eq!(last.unix_seconds(), 253_402_300_799);
    assert_eq!(UtcInstant::from_unix_seconds(253_402_300_799)?, last);
    for out_of_range in [253_402_300_800, u64::MAX] {
        assert_eq!(
            UtcInstant::from_unix_seconds(out_of_range).err(),
            Some(QualityArtifactError::InvalidTimestamp),
            "{out_of_range}"
        );
    }
    assert_eq!(
        last.checked_add_seconds(1).err(),
        Some(QualityArtifactError::InvalidTimestamp)
    );

    // 2100 is a century that is not a leap year; 2000 and 2024 are.
    assert_eq!(
        "2100-02-29T00:00:00Z".parse::<UtcInstant>().err(),
        Some(QualityArtifactError::InvalidTimestamp)
    );
    assert_eq!(
        "2100-03-01T00:00:00Z".parse::<UtcInstant>()?.unix_seconds(),
        4_107_542_400
    );
    assert!("2000-02-29T00:00:00Z".parse::<UtcInstant>().is_ok());
    assert!("2024-02-29T00:00:00Z".parse::<UtcInstant>().is_ok());
    assert_eq!(
        "2023-02-29T00:00:00Z".parse::<UtcInstant>().err(),
        Some(QualityArtifactError::InvalidTimestamp)
    );
    Ok(())
}

#[test]
fn capacity_floor_reserves_the_m2_recovery_headroom_exactly() {
    let request = 64 * 1024 * 1024;
    let floor = request + M2_RECOVERY_HEADROOM_BYTES + QUALITY_CONTROL_HEADROOM_BYTES;
    assert_eq!(M2_RECOVERY_HEADROOM_BYTES, 49 * 1024 * 1024);
    assert_eq!(reservation_fits(floor, request), Ok(()));
    assert_eq!(
        reservation_fits(floor - 1, request),
        Err(QualityArtifactError::QuotaExceeded)
    );
    assert_eq!(
        reservation_fits(u64::MAX, u64::MAX),
        Err(QualityArtifactError::QuotaExceeded)
    );
}

#[test]
fn descriptors_and_watermarks_reject_unknown_fields() -> Check {
    let created = UtcInstant::from_unix_seconds(1_788_000_000)?;
    let encoded = serde_json::to_string(&descriptor(&created, 3_600)?)?;
    assert!(serde_json::from_str::<QualityArtifactDescriptor>(&encoded).is_ok());
    let injected = encoded.replace("{\"format_version\"", "{\"extra\":1,\"format_version\"");
    assert!(serde_json::from_str::<QualityArtifactDescriptor>(&injected).is_err());

    let watermark = QualityClockWatermark::new(created);
    assert_eq!(watermark.validate(), Ok(()));
    let encoded = serde_json::to_string(&watermark)?;
    assert_eq!(
        serde_json::from_str::<QualityClockWatermark>(&encoded)?,
        watermark
    );
    assert!(
        serde_json::from_str::<QualityClockWatermark>(
            "{\"format_version\":1,\"observed_at_utc\":\"2026-09-06T00:00:00Z\",\"x\":1}"
        )
        .is_err()
    );
    // A non-canonical instant never becomes a durable watermark.
    assert!(
        serde_json::from_str::<QualityClockWatermark>(
            "{\"format_version\":1,\"observed_at_utc\":\"2026-09-06T00:00:00+00:00\"}"
        )
        .is_err()
    );
    Ok(())
}
