//! Strict, durable quality-artifact descriptors.  These values carry no host paths.
//!
//! ADR-061 keeps this module free of clocks, filesystems, URI text and archive
//! parsing: it only decides whether a candidate descriptor is well formed.
use crate::job::JobId;
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt, str::FromStr};

fn canonical_id(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|suffix| {
        suffix.len() == 32
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

macro_rules! opaque_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
            /// Canonical locator from 128 bits of caller-supplied entropy.
            pub fn from_random_bytes(bytes: [u8; 16]) -> Self {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let mut encoded = String::with_capacity($prefix.len() + 32);
                encoded.push_str($prefix);
                for byte in bytes {
                    encoded.push(char::from(HEX[usize::from(byte >> 4)]));
                    encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
                }
                Self(encoded)
            }
        }
        impl TryFrom<String> for $name {
            type Error = QualityArtifactError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                canonical_id(&value, $prefix)
                    .then_some(Self(value))
                    .ok_or(QualityArtifactError::InvalidId)
            }
        }
        impl FromStr for $name {
            type Err = QualityArtifactError;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                value.to_owned().try_into()
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

opaque_id!(QualityArtifactId, "qart_");
/// ADR-061 uses ADR-060's canonical job locator without a second ID grammar.
pub type QualityJobId = JobId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualityArtifactError {
    InvalidId,
    InvalidDescriptor,
    InvalidTimestamp,
    InvalidKindVersion,
    InvalidLimit,
    NotFound,
    Unauthorized,
    Expired,
    QuotaExceeded,
    Busy,
    UnsupportedPlatform,
    /// The host state root exists but does not qualify for a durable store:
    /// it is not this uid's directory, or it is group- or world-writable.
    UnsupportedStateRoot,
    Io,
    RecoveryRequired,
    RetentionDenied,
}
impl fmt::Display for QualityArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidId => "invalid quality artifact identifier",
            Self::InvalidDescriptor => "invalid quality artifact descriptor",
            Self::InvalidTimestamp => "invalid quality artifact timestamp",
            Self::InvalidKindVersion => "invalid quality artifact kind/version",
            Self::InvalidLimit => "invalid quality artifact limit",
            Self::NotFound => "quality artifact not found",
            Self::Unauthorized => "quality artifact authorization failed",
            Self::Expired => "quality artifact expired",
            Self::QuotaExceeded => "quality artifact quota exceeded",
            Self::Busy => "quality artifact store busy",
            Self::UnsupportedPlatform => "quality artifact store unsupported platform",
            Self::UnsupportedStateRoot => "quality artifact store unqualified state root",
            Self::Io => "quality artifact store I/O failed",
            Self::RecoveryRequired => "quality artifact recovery required",
            Self::RetentionDenied => "quality artifact retention not granted",
        })
    }
}
impl Error for QualityArtifactError {}

const SECONDS_PER_DAY: u64 = 86_400;

/// Days between 1970-01-01 and the given proleptic Gregorian date (Howard
/// Hinnant's `days_from_civil`). Pure integer arithmetic, no calendar crate.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_index = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// A single canonical observational instant: exactly `YYYY-MM-DDTHH:MM:SSZ`.
///
/// Only one spelling of an instant is accepted so that stored descriptors, the
/// durable clock watermark and expiry comparisons cannot disagree about a value.
/// Offsets, fractional seconds and leap seconds are rejected rather than folded.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct UtcInstant(String);
impl UtcInstant {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Seconds since the Unix epoch. Total because construction validated it.
    pub fn unix_seconds(&self) -> u64 {
        let field = |range: std::ops::Range<usize>| -> i64 {
            self.0
                .get(range)
                .and_then(|text| text.parse::<i64>().ok())
                .unwrap_or_default()
        };
        let days = days_from_civil(field(0..4), field(5..7), field(8..10));
        let seconds = days * SECONDS_PER_DAY as i64
            + field(11..13) * 3_600
            + field(14..16) * 60
            + field(17..19);
        u64::try_from(seconds).unwrap_or_default()
    }
    pub fn from_unix_seconds(seconds: u64) -> Result<Self, QualityArtifactError> {
        let days = i64::try_from(seconds / SECONDS_PER_DAY)
            .map_err(|_| QualityArtifactError::InvalidTimestamp)?;
        let rest = seconds % SECONDS_PER_DAY;
        let (year, month, day) = civil_from_days(days);
        if !(1970..=9999).contains(&year) {
            return Err(QualityArtifactError::InvalidTimestamp);
        }
        Ok(Self(format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
            rest / 3_600,
            (rest % 3_600) / 60,
            rest % 60
        )))
    }
    pub fn checked_add_seconds(&self, seconds: u64) -> Result<Self, QualityArtifactError> {
        self.unix_seconds()
            .checked_add(seconds)
            .ok_or(QualityArtifactError::InvalidTimestamp)
            .and_then(Self::from_unix_seconds)
    }
    /// Non-negative distance to a later instant; `None` when `self` is later.
    pub fn seconds_until(&self, later: &Self) -> Option<u64> {
        later.unix_seconds().checked_sub(self.unix_seconds())
    }
}
impl TryFrom<String> for UtcInstant {
    type Error = QualityArtifactError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let bytes = value.as_bytes();
        let digits = |range: std::ops::Range<usize>| -> Option<i64> {
            let text = value.get(range)?;
            text.bytes()
                .all(|byte| byte.is_ascii_digit())
                .then(|| text.parse().ok())
                .flatten()
        };
        if bytes.len() != 20
            || bytes.get(4) != Some(&b'-')
            || bytes.get(7) != Some(&b'-')
            || bytes.get(10) != Some(&b'T')
            || bytes.get(13) != Some(&b':')
            || bytes.get(16) != Some(&b':')
            || bytes.get(19) != Some(&b'Z')
        {
            return Err(QualityArtifactError::InvalidTimestamp);
        }
        let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
            digits(0..4),
            digits(5..7),
            digits(8..10),
            digits(11..13),
            digits(14..16),
            digits(17..19),
        ) else {
            return Err(QualityArtifactError::InvalidTimestamp);
        };
        if !(1970..=9999).contains(&year)
            || !(1..=12).contains(&month)
            || day < 1
            || hour > 23
            || minute > 59
            || second > 59
        {
            return Err(QualityArtifactError::InvalidTimestamp);
        }
        let seconds = days_from_civil(year, month, day) * SECONDS_PER_DAY as i64
            + hour * 3_600
            + minute * 60
            + second;
        let seconds = u64::try_from(seconds).map_err(|_| QualityArtifactError::InvalidTimestamp)?;
        // Re-rendering rejects out-of-range days (2026-02-30) and any spelling
        // that is not already canonical, without a second calendar table.
        let canonical = Self::from_unix_seconds(seconds)?;
        (canonical.0 == value)
            .then_some(canonical)
            .ok_or(QualityArtifactError::InvalidTimestamp)
    }
}
impl FromStr for UtcInstant {
    type Err = QualityArtifactError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}
impl From<UtcInstant> for String {
    fn from(value: UtcInstant) -> Self {
        value.0
    }
}
impl fmt::Display for UtcInstant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Durable monotonicity witness for the quality store only. A durable instant
/// later than the observed wall clock is a regression that fails the store
/// closed; it never rewrites M1 or M2 state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityClockWatermark {
    pub format_version: u8,
    pub observed_at_utc: UtcInstant,
}
impl QualityClockWatermark {
    pub fn new(observed_at_utc: UtcInstant) -> Self {
        Self {
            format_version: 1,
            observed_at_utc,
        }
    }
    pub fn validate(&self) -> Result<(), QualityArtifactError> {
        (self.format_version == 1)
            .then_some(())
            .ok_or(QualityArtifactError::InvalidDescriptor)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityArtifactKind {
    JunitXml,
    CoverageJson,
    Lcov,
    ArchiveBundle,
    MutationDiff,
    MutationLog,
    ToolLog,
    OtherDeclared,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityMimeType {
    ApplicationJunitXml,
    ApplicationJson,
    TextPlain,
    TextXDiff,
    ApplicationXTar,
    ApplicationOctetStream,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayloadFormatVersion {
    JunitXmlV1,
    CoverageJsonV1,
    LcovV1,
    UstarV1,
    MutationDiffV1,
    Utf8LogV1,
    DeclaredV1,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactCompleteness {
    Complete,
    Truncated,
    Partial,
    Invalid,
    Unavailable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactSensitivity {
    Public,
    SourceDerived,
    SymbolDerived,
    PotentiallySensitive,
    SecretSuspected,
}
/// The fixed guest table. A guest string never becomes one of these values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestArtifactName {
    JunitXml,
    CoverageJson,
    Lcov,
    OutcomesJson,
    MutationDiff,
    MutationLog,
    ToolLog,
    ReportArchive,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactSource {
    pub captured_source_sha256: [u8; 32],
    pub guest_name: GuestArtifactName,
    pub selection: ArtifactSelection,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactSelection {
    Workspace,
    Package,
    Target,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRuntime {
    pub image_digest: [u8; 32],
    pub toolchain_identity: [u8; 32],
    pub plugin: ArtifactPlugin,
    pub implementation_digest: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactPlugin {
    pub identity: PluginIdentity,
    pub version: u16,
    pub digest: [u8; 32],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginIdentity {
    Builtin,
    Nextest,
    Coverage,
    Semver,
    Mutation,
}

/// Everything a producer declares before its bytes exist. Size and digest are
/// deliberately absent: only the store that consumed the stream may state them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualityArtifactDraft {
    pub artifact_id: QualityArtifactId,
    pub member_index: u16,
    pub kind: QualityArtifactKind,
    pub mime_type: QualityMimeType,
    pub payload_format_version: PayloadFormatVersion,
    pub completeness: ArtifactCompleteness,
    pub sensitivity: ArtifactSensitivity,
    pub created_at_utc: UtcInstant,
    pub expires_at_utc: UtcInstant,
    pub source: ArtifactSource,
    pub runtime: ArtifactRuntime,
}
impl QualityArtifactDraft {
    pub fn into_descriptor(
        self,
        job_id: QualityJobId,
        owner_binding: [u8; 32],
        sha256: [u8; 32],
        size_bytes: u64,
    ) -> Result<QualityArtifactDescriptor, QualityArtifactError> {
        let descriptor = QualityArtifactDescriptor {
            format_version: 1,
            artifact_id: self.artifact_id,
            job_id,
            member_index: self.member_index,
            kind: self.kind,
            mime_type: self.mime_type,
            payload_format_version: self.payload_format_version,
            sha256,
            size_bytes,
            completeness: self.completeness,
            sensitivity: self.sensitivity,
            created_at_utc: self.created_at_utc,
            expires_at_utc: self.expires_at_utc,
            owner_binding,
            source: self.source,
            runtime: self.runtime,
        };
        descriptor.validate()?;
        Ok(descriptor)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityArtifactDescriptor {
    pub format_version: u8,
    pub artifact_id: QualityArtifactId,
    pub job_id: QualityJobId,
    pub member_index: u16,
    pub kind: QualityArtifactKind,
    pub mime_type: QualityMimeType,
    pub payload_format_version: PayloadFormatVersion,
    pub sha256: [u8; 32],
    pub size_bytes: u64,
    pub completeness: ArtifactCompleteness,
    pub sensitivity: ArtifactSensitivity,
    pub created_at_utc: UtcInstant,
    pub expires_at_utc: UtcInstant,
    pub owner_binding: [u8; 32],
    pub source: ArtifactSource,
    pub runtime: ArtifactRuntime,
}
impl QualityArtifactDescriptor {
    /// Total structural validation. It proves nothing about stored bytes; the
    /// adapter still revalidates identity, size and digest before serving.
    pub fn validate(&self) -> Result<(), QualityArtifactError> {
        if self.format_version != 1
            || self.size_bytes > QUALITY_MAX_ARTIFACT_BYTES
            || self.member_index >= QUALITY_MAX_JOB_MEMBERS
        {
            return Err(QualityArtifactError::InvalidDescriptor);
        }
        match self
            .created_at_utc
            .seconds_until(&self.expires_at_utc)
            .ok_or(QualityArtifactError::InvalidTimestamp)?
        {
            0 => return Err(QualityArtifactError::InvalidTimestamp),
            ttl if ttl > QUALITY_MAX_TTL_SECONDS => {
                return Err(QualityArtifactError::InvalidTimestamp);
            }
            _ => {}
        }
        let kind_version = matches!(
            (self.kind, self.payload_format_version),
            (
                QualityArtifactKind::JunitXml,
                PayloadFormatVersion::JunitXmlV1
            ) | (
                QualityArtifactKind::CoverageJson,
                PayloadFormatVersion::CoverageJsonV1
            ) | (QualityArtifactKind::Lcov, PayloadFormatVersion::LcovV1)
                | (
                    QualityArtifactKind::ArchiveBundle,
                    PayloadFormatVersion::UstarV1
                )
                | (
                    QualityArtifactKind::MutationDiff,
                    PayloadFormatVersion::MutationDiffV1
                )
                | (
                    QualityArtifactKind::MutationLog | QualityArtifactKind::ToolLog,
                    PayloadFormatVersion::Utf8LogV1
                )
                | (
                    QualityArtifactKind::OtherDeclared,
                    PayloadFormatVersion::DeclaredV1
                )
        );
        let kind_mime = matches!(
            (self.kind, self.mime_type),
            (
                QualityArtifactKind::JunitXml,
                QualityMimeType::ApplicationJunitXml
            ) | (
                QualityArtifactKind::CoverageJson,
                QualityMimeType::ApplicationJson
            ) | (
                QualityArtifactKind::Lcov
                    | QualityArtifactKind::MutationLog
                    | QualityArtifactKind::ToolLog,
                QualityMimeType::TextPlain
            ) | (
                QualityArtifactKind::ArchiveBundle,
                QualityMimeType::ApplicationXTar
            ) | (
                QualityArtifactKind::MutationDiff,
                QualityMimeType::TextXDiff
            ) | (
                QualityArtifactKind::OtherDeclared,
                QualityMimeType::ApplicationOctetStream
            )
        );
        let kind_guest = matches!(
            (self.kind, self.source.guest_name),
            (QualityArtifactKind::JunitXml, GuestArtifactName::JunitXml)
                | (
                    QualityArtifactKind::CoverageJson,
                    GuestArtifactName::CoverageJson
                )
                | (QualityArtifactKind::Lcov, GuestArtifactName::Lcov)
                | (
                    QualityArtifactKind::ArchiveBundle,
                    GuestArtifactName::ReportArchive
                )
                | (
                    QualityArtifactKind::MutationDiff,
                    GuestArtifactName::MutationDiff
                )
                | (
                    QualityArtifactKind::MutationLog,
                    GuestArtifactName::MutationLog
                )
                | (QualityArtifactKind::ToolLog, GuestArtifactName::ToolLog)
                | (
                    QualityArtifactKind::OtherDeclared,
                    GuestArtifactName::OutcomesJson
                )
        );
        (kind_version && kind_mime && kind_guest)
            .then_some(())
            .ok_or(QualityArtifactError::InvalidKindVersion)
    }
    /// Expiry is observational: `now` is supplied by the caller's hybrid clock.
    pub fn is_expired(&self, now: &UtcInstant) -> bool {
        now.unix_seconds() >= self.expires_at_utc.unix_seconds()
    }
}

/// Closed quarantine reasons. Never a path, a URI or attacker-supplied text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuarantineReason {
    UnknownName,
    UnknownVersion,
    MalformedDescriptor,
    NotPrivateRegularFile,
    SizeMismatch,
    DigestMismatch,
    MissingBlob,
    ClockAnomaly,
}
impl fmt::Display for QuarantineReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnknownName => "unknown_name",
            Self::UnknownVersion => "unknown_version",
            Self::MalformedDescriptor => "malformed_descriptor",
            Self::NotPrivateRegularFile => "not_private_regular_file",
            Self::SizeMismatch => "size_mismatch",
            Self::DigestMismatch => "digest_mismatch",
            Self::MissingBlob => "missing_blob",
            Self::ClockAnomaly => "clock_anomaly",
        })
    }
}

/// Bounded counts only: a report never carries a name, path or byte of content.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryReport {
    pub validated: u32,
    pub discarded_uncommitted: u32,
    pub quarantined: u32,
    pub truncated_surplus: u32,
    pub released_reservations: u32,
    pub clock_regression: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PruneReport {
    pub removed: u32,
    pub reclaimed_bytes: u64,
    pub retained: u32,
}

pub const QUALITY_MAX_ARTIFACT_BYTES: u64 = 32 * 1024 * 1024;
pub const QUALITY_MAX_JOB_BYTES: u64 = 64 * 1024 * 1024;
pub const QUALITY_MAX_JOB_MEMBERS: u16 = 128;
pub const QUALITY_MAX_OWNER_BYTES: u64 = 128 * 1024 * 1024;
pub const QUALITY_MAX_GLOBAL_BYTES: u64 = 256 * 1024 * 1024;
pub const QUALITY_DEFAULT_TTL_SECONDS: u64 = 3_600;
pub const QUALITY_MAX_TTL_SECONDS: u64 = 86_400;
pub const QUALITY_CONTROL_HEADROOM_BYTES: u64 = 16 * 1024 * 1024;
/// M2's own floor, mirrored from `RECOVERY_STAGING_HEADROOM_BYTES +
/// RETAINED_METADATA_GROWTH_BYTES` in the macOS mutation store (48 MiB
/// recovery staging + 1 MiB metadata growth). A maximal quality reservation
/// must leave it available so an M2 commit still succeeds afterwards.
pub const M2_RECOVERY_HEADROOM_BYTES: u64 = 49 * 1024 * 1024;

/// The state-root capacity floor, kept pure so its boundary is testable
/// without a real volume. The caller supplies observed free bytes.
pub fn reservation_fits(free_bytes: u64, requested: u64) -> Result<(), QualityArtifactError> {
    let required = requested
        .checked_add(M2_RECOVERY_HEADROOM_BYTES)
        .and_then(|value| value.checked_add(QUALITY_CONTROL_HEADROOM_BYTES))
        .ok_or(QualityArtifactError::QuotaExceeded)?;
    (free_bytes >= required)
        .then_some(())
        .ok_or(QualityArtifactError::QuotaExceeded)
}
/// Bounds every directory walk so a hostile or damaged store cannot make
/// reconciliation, accounting or paging unbounded work.
pub const QUALITY_MAX_STORE_ENTRIES: usize = 4_096;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opaque_ids_are_canonical() {
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
            "qart_0123456789abcdef0123456789abcdeF"
                .parse::<QualityArtifactId>()
                .is_err()
        );
        assert_eq!(
            QualityArtifactId::from_random_bytes([0xab; 16]).as_str(),
            "qart_abababababababababababababababab"
        );
    }
    #[test]
    fn instants_accept_exactly_one_canonical_spelling() {
        assert_eq!(
            "1970-01-01T00:00:00Z"
                .parse::<UtcInstant>()
                .map(|value| value.unix_seconds()),
            Ok(0)
        );
        for offset in [0_u64, 1, 86_399, 86_400, 951_782_400, 1_788_697_496] {
            let instant = UtcInstant::from_unix_seconds(offset);
            assert_eq!(
                instant.clone().map(|value| value.unix_seconds()),
                Ok(offset)
            );
            assert_eq!(
                instant
                    .clone()
                    .map(String::from)
                    .and_then(UtcInstant::try_from),
                instant
            );
        }
        for bad in [
            "2026-09-06T12:34:56+00:00",
            "2026-09-06T12:34:56.5Z",
            "2026-02-30T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-09-06T24:00:00Z",
            "2026-09-06T12:34:60Z",
            "1969-12-31T23:59:59Z",
            "2026-9-06T12:34:56Z",
        ] {
            assert_eq!(
                bad.parse::<UtcInstant>().err(),
                Some(QualityArtifactError::InvalidTimestamp),
                "{bad}"
            );
        }
    }
}
